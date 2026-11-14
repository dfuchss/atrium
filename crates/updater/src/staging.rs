//! Downloads a release asset into `<app data>/updates/<tag>/`, verifies it against the
//! release's `SHA256SUMS`, extracts and checks the bundle and moves it next to the install
//! (`<install>.new-update`). The install itself is never written.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest as _, Sha256};

use crate::release::{Release, SHA256SUMS, parse_sha256sums};
use crate::source::ReleaseSource;
use crate::swap::{new_update_path, partial_update_path};
use crate::{archive, bundle, quarantine};

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
    let zip = dir.join(request.asset_name);
    let actual = download(source, asset_url, &zip, on_progress)?;
    if !actual.eq_ignore_ascii_case(&expected) {
        let _ = std::fs::remove_file(&zip);
        return Err(format!("checksum mismatch for {}", request.asset_name));
    }

    let extracted = dir.join("extracted");
    archive::extract(&zip, &extracted)?;
    let bundle = bundle::find_bundle(&extracted)?;
    bundle::verify(&bundle, release.version)?;
    quarantine::remove_quarantine(&bundle)?;
    let staged = place_next_to_install(&bundle, request.install)?;
    let _ = std::fs::remove_dir_all(&extracted);
    let _ = std::fs::remove_file(&zip);
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

/// Moves the checked bundle to `<install>.new-update`, replacing a leftover. Across volumes
/// it is copied with `ditto` to a `.partial` sibling first, so a broken copy never looks
/// complete; the copy is cleared of the quarantine `ditto` may have set again.
fn place_next_to_install(bundle: &Path, install: &Path) -> Result<PathBuf, String> {
    let target = new_update_path(install);
    let partial = partial_update_path(install);
    remove_if_present(&partial)?;
    remove_if_present(&target)?;
    if std::fs::rename(bundle, &target).is_ok() {
        return Ok(target);
    }
    let copied = Command::new("/usr/bin/ditto")
        .arg(bundle)
        .arg(&partial)
        .status()
        .is_ok_and(|status| status.success());
    if copied
        && quarantine::remove_quarantine(&partial).is_ok()
        && std::fs::rename(&partial, &target).is_ok()
    {
        return Ok(target);
    }
    let _ = remove_if_present(&partial);
    Err("cannot place the update next to the installation".into())
}

/// Removes a file or folder tree; a missing path is fine.
pub(crate) fn remove_if_present(path: &Path) -> Result<(), String> {
    let Ok(meta) = path.symlink_metadata() else {
        return Ok(());
    };
    let removed = if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    removed.map_err(|error| format!("cannot remove {}: {error}", path.display()))
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

    #[test]
    fn removing_a_missing_path_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        remove_if_present(&dir.path().join("missing")).unwrap();
        std::fs::create_dir_all(dir.path().join("tree/sub")).unwrap();
        remove_if_present(&dir.path().join("tree")).unwrap();
        assert!(!dir.path().join("tree").exists());
    }
}
