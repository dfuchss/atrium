//! Downloads of service views: saved into the user's Downloads folder, never over an existing
//! file and never as a hidden one.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use domain::{Input, redact_url};
use fs_util::lock_ignoring_poison;
use tauri::webview::DownloadEvent;
use tauri::{AppHandle, Manager, Runtime, Url, Webview};
use uuid::Uuid;

use crate::{Feed, Logger};

/// `dir/name`, or `dir/stem (n).ext` for the first `n` that does not exist yet, so a download
/// never replaces an existing file.
fn unused_path(dir: &Path, name: &OsStr) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let original = Path::new(name);
    let stem = original.file_stem().unwrap_or(name).to_string_lossy();
    let extension = original.extension().map_or_else(String::new, |extension| {
        format!(".{}", extension.to_string_lossy())
    });
    (1..u32::MAX)
        .map(|n| dir.join(format!("{stem} ({n}){extension}")))
        .find(|path| !path.exists())
        .unwrap_or(candidate)
}

/// Name a download is saved under: the file name the page suggested, or None for hidden
/// (dot) files, which would land unseen in Downloads (e.g. `.zshrc`, `.htaccess`).
fn download_file_name(suggested: &Path) -> Option<String> {
    let name = suggested.file_name().map_or_else(
        || "download".into(),
        |name| name.to_string_lossy().into_owned(),
    );
    (!name.starts_with('.')).then_some(name)
}

/// Download handler: saves into the user's Downloads folder (quarantined by macOS, see
/// `LSFileQuarantineEnabled` in Info.plist), refuses hidden files and reports finished
/// downloads as [`Input::DownloadFinished`]. On macOS `Finished` carries no path, so the name
/// is remembered per URL from `Requested`.
pub(crate) fn download_hook<R: Runtime>(
    app: AppHandle<R>,
    account: Uuid,
    logger: Logger,
    feed: Feed,
) -> impl Fn(Webview<R>, DownloadEvent<'_>) -> bool + Send + Sync + 'static {
    let pending: Mutex<HashMap<Url, String>> = Mutex::default();
    move |_, event| match event {
        DownloadEvent::Requested { url, destination } => {
            let shown_url = redact_url(url.as_str());
            let Some(name) = download_file_name(destination) else {
                logger.log(&format!(
                    "[{account}] download {shown_url} refused: hidden file name"
                ));
                return false;
            };
            if let Ok(dir) = app.path().download_dir() {
                *destination = unused_path(&dir, OsStr::new(&name));
            }
            logger.log(&format!(
                "[{account}] download {shown_url} -> {}",
                destination.display()
            ));
            lock_ignoring_poison(&pending).insert(url, name);
            true
        }
        DownloadEvent::Finished { url, success, .. } => {
            logger.log(&format!(
                "[{account}] download finished {} ok={success}",
                redact_url(url.as_str())
            ));
            let name = lock_ignoring_poison(&pending).remove(&url);
            if let (true, Some(file_name)) = (success, name) {
                feed(Input::DownloadFinished { account, file_name });
            }
            true
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_downloads_are_refused() {
        assert_eq!(
            download_file_name(Path::new("/tmp/report.pdf")).as_deref(),
            Some("report.pdf")
        );
        assert_eq!(
            download_file_name(Path::new("/")).as_deref(),
            Some("download")
        );
        assert_eq!(download_file_name(Path::new("/tmp/.zshrc")), None);
        assert_eq!(download_file_name(Path::new(".hidden")), None);
    }

    #[test]
    fn downloads_never_replace_existing_files() {
        let dir = std::env::temp_dir().join(format!("atrium-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let name = OsStr::new("report.pdf");
        assert_eq!(unused_path(&dir, name), dir.join("report.pdf"));
        std::fs::write(dir.join("report.pdf"), "x").unwrap();
        assert_eq!(unused_path(&dir, name), dir.join("report (1).pdf"));
        std::fs::write(dir.join("report (1).pdf"), "x").unwrap();
        assert_eq!(unused_path(&dir, name), dir.join("report (2).pdf"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
