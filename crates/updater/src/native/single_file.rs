//! What the platforms that update a single file (the `AppImage` on Linux, the executable on
//! Windows) share: the file is its own helper, nothing else runs from it and nobody relaunches.

use std::path::{Path, PathBuf};

use crate::staging::{copy_to_new_file, place_next_to_install};
use crate::version::Version;

/// Why a release without the file for this machine is only announced.
pub(crate) const NO_DOWNLOAD_REASON: &str = "the release has no download for this computer";

/// Moves the verified download next to the install (`<install>.new-update`, executable on
/// Unix), which it returns. Identity and version come from `SHA256SUMS` and the tag.
pub(crate) fn prepare_update(
    downloaded: &Path,
    _version: Version,
    install: &Path,
) -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let mode = crate::staging::EXECUTABLE_MODE;
        std::fs::set_permissions(downloaded, std::fs::Permissions::from_mode(mode))
            .map_err(|error| format!("cannot make the update executable: {error}"))?;
    }
    place_next_to_install(downloaded, install, copy_to_new_file)
}

/// The helper is the installed file itself (on Linux started through the `AppImage`: the
/// running executable lives in its mount, which goes away when the app exits).
#[expect(
    clippy::unnecessary_wraps,
    reason = "the platforms share this signature; macOS can fail"
)]
pub(crate) fn helper_executable(install: &Path) -> std::io::Result<PathBuf> {
    Ok(install.to_path_buf())
}

/// Always empty: renaming the file while another instance runs from it is safe; that
/// instance keeps the old version until it restarts.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the platforms share this signature; macOS can fail"
)]
pub(crate) fn processes_running_from(_install: &Path) -> Result<Vec<u32>, String> {
    Ok(Vec::new())
}

/// Nothing to do: the user opens Atrium again after the swap.
pub(crate) fn relaunch(_install: &Path) {}

/// Fakes of the installation and the asset the platform tests share.
#[cfg(test)]
pub(crate) mod fakes {
    use super::*;
    use crate::swap::new_update_path;

    /// The release asset of Atrium in `version`: a file holding the version.
    pub(crate) fn fake_asset(_dir: &Path, version: &str) -> Vec<u8> {
        version.as_bytes().to_vec()
    }

    /// Stages the single file of `version` next to `install`.
    pub(crate) fn fake_staged(dir: &Path, install: &Path, version: &str) {
        let staged = new_update_path(install);
        std::fs::write(&staged, fake_asset(dir, version)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let mode = crate::staging::EXECUTABLE_MODE;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(mode)).unwrap();
        }
    }

    /// Fails unless `path` is the single file of `version` (executable on Unix).
    pub(crate) fn verify_staged(path: &Path, version: &str) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let mode = std::fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o777,
                crate::staging::EXECUTABLE_MODE,
                "{}",
                path.display()
            );
        }
        assert_eq!(std::fs::read(path).unwrap(), fake_asset(path, version));
    }
}
