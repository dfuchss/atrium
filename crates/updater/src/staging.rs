//! Downloads a release asset into `<app data>/updates/<tag>/`, verifies it against the
//! release's `SHA256SUMS` and has the platform place it next to the install
//! (`<install>.new-update`) with `place_next_to_install`. The install itself is never written.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::native;
use crate::release::{Release, SHA256SUMS, parse_sha256sums};
use crate::source::ReleaseSource;

const MAX_SUMS_BYTES: u64 = 64 * 1024;
const MAX_ASSET_BYTES: u64 = 512 * 1024 * 1024;
const BUFFER_BYTES: usize = 64 * 1024;

/// What to stage and where.
pub(crate) struct StageRequest<'a> {
    pub(crate) release: &'a Release,
    pub(crate) asset_name: &'a str,
    pub(crate) install: &'a Path,
    pub(crate) updates_dir: &'a Path,
}

/// Stages the update and returns `<install>.new-update`. `on_progress` gets the download
/// progress in percent (`None` while the size is unknown).
pub(crate) fn stage(
    source: &dyn ReleaseSource,
    request: &StageRequest<'_>,
    on_progress: &mut dyn FnMut(Option<u8>),
) -> Result<PathBuf, String> {
    let release = request.release;
    let asset_url = release
        .assets
        .get(request.asset_name)
        .ok_or_else(|| format!("the release has no {}", request.asset_name))?;
    let expected = expected_digest(source, release, request.asset_name)?;

    let dir = request.updates_dir.join(release.version.tag());
    remove_if_present(&dir)?;
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let downloaded = dir.join(request.asset_name);
    let actual = download(source, asset_url, &downloaded, on_progress)?;
    if !actual.eq_ignore_ascii_case(&expected) {
        let _ = std::fs::remove_file(&downloaded);
        return Err(format!("checksum mismatch for {}", request.asset_name));
    }

    let staged = native::prepare_update(&downloaded, release.version, request.install)?;
    let _ = std::fs::remove_file(&downloaded);
    Ok(staged)
}

/// The digest of `asset_name` from the release's `SHA256SUMS`; a release without the file or
/// without an entry is refused.
fn expected_digest(
    source: &dyn ReleaseSource,
    release: &Release,
    asset_name: &str,
) -> Result<String, String> {
    let sums_url = release
        .assets
        .get(SHA256SUMS)
        .ok_or_else(|| format!("the release has no {SHA256SUMS}"))?;
    let mut sums = String::new();
    source
        .open(sums_url, MAX_SUMS_BYTES)?
        .reader
        .read_to_string(&mut sums)
        .map_err(|error| format!("cannot read {SHA256SUMS}: {error}"))?;
    parse_sha256sums(&sums)
        .remove(asset_name)
        .ok_or_else(|| format!("{SHA256SUMS} has no entry for {asset_name}"))
}

/// Downloads to `<target>.part`, renames it to `target` and returns the SHA-256 hex digest.
fn download(
    source: &dyn ReleaseSource,
    url: &str,
    target: &Path,
    on_progress: &mut dyn FnMut(Option<u8>),
) -> Result<String, String> {
    let mut download = source.open(url, MAX_ASSET_BYTES)?;
    let partial = target.with_extension("part");
    let mut file = File::create(&partial).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; BUFFER_BYTES];
    let (mut received, mut reported) = (0u64, None);
    on_progress(percent(0, download.length));
    loop {
        let count = download
            .reader
            .read(&mut buffer)
            .map_err(|error| format!("download failed: {error}"))?;
        if count == 0 {
            break;
        }
        file.write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
        digest.update(&buffer[..count]);
        received += count as u64;
        let progress = percent(received, download.length);
        if progress != reported {
            reported = progress;
            on_progress(progress);
        }
    }
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&partial, target).map_err(|error| error.to_string())?;
    Ok(hex(&digest.finalize()))
}

/// Lower case hex digits of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn percent(received: u64, length: Option<u64>) -> Option<u8> {
    let length = length.filter(|&length| length > 0)?;
    u8::try_from((received.min(length) * 100) / length).ok()
}

/// Mode of a staged single-file update: it has to run after the swap.
#[cfg(not(target_os = "macos"))]
pub(crate) const EXECUTABLE_MODE: u32 = 0o755;

/// Moves the checked update `update` to `<install>.new-update`, replacing a leftover. Across
/// file systems `copy` writes it to a new `.partial` sibling first (it is synced or complete
/// when `copy` returns), so a broken copy never looks complete. The folder is synced last, so
/// the staged update outlives a crash.
pub(crate) fn place_next_to_install(
    update: &Path,
    install: &Path,
    copy: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<PathBuf, String> {
    use crate::swap::{new_update_path, partial_update_path};

    let target = new_update_path(install);
    let partial = partial_update_path(install);
    remove_if_present(&partial)?;
    remove_if_present(&target)?;
    let placed = std::fs::rename(update, &target).or_else(|_| {
        copy(update, &partial)
            .and_then(|()| std::fs::rename(&partial, &target))
            .inspect_err(|_| {
                let _ = remove_if_present(&partial);
            })
    });
    placed
        .and_then(|()| sync_folder_of(&target))
        .map_err(|error| format!("cannot place the update next to the installation: {error}"))?;
    Ok(target)
}

/// Copies `from` to the executable file `to`, which must not exist yet (so a file or link
/// someone else put there is never written through), and syncs it.
#[cfg(not(target_os = "macos"))]
pub(crate) fn copy_to_new_file(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut source = File::open(from)?;
    let mut copy = fs_util::open_with_mode(
        std::fs::OpenOptions::new().write(true).create_new(true),
        to,
        EXECUTABLE_MODE,
    )?;
    std::io::copy(&mut source, &mut copy)?;
    copy.sync_all()
}

/// Syncs the folder that holds `path`, so a rename into it is on disk.
fn sync_folder_of(path: &Path) -> std::io::Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no folder"))?;
    fs_util::sync_folder(folder)
}

/// Removes a file or folder tree; a missing path is fine.
pub(crate) fn remove_if_present(path: &Path) -> Result<(), String> {
    fs_util::remove_if_present(path)
        .map_err(|error| format!("cannot remove {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_is_bounded_and_needs_a_length() {
        assert_eq!(percent(0, Some(200)), Some(0));
        assert_eq!(percent(100, Some(200)), Some(50));
        assert_eq!(percent(300, Some(200)), Some(100));
        assert_eq!(percent(10, None), None);
        assert_eq!(percent(10, Some(0)), None);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn copies_only_to_a_new_executable_file() {
        use crate::native::tests::{fake_asset, verify_staged};

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("download");
        std::fs::write(&source, fake_asset(dir.path(), "0.2.0")).unwrap();
        let copy = dir.path().join("copy");
        copy_to_new_file(&source, &copy).unwrap();
        verify_staged(&copy, "0.2.0");
        let error = copy_to_new_file(&source, &copy).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "keep").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        assert!(copy_to_new_file(&source, &link).is_err());
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    }
}
