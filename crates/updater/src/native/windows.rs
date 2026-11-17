//! Windows: the release is a single executable, `atrium-windows-x86_64.exe`, but Atrium does not
//! update itself on Windows yet: every installation is updated by hand, so the release is only
//! announced. Staging already works like on Linux (a single file next to the installation); the
//! swap helper never runs.

use std::path::{Path, PathBuf};

use crate::location::{InstallLocation, RunningAppImage, can_replace};
use crate::staging::place_next_to_install;
use crate::version::Version;

/// The user opens Atrium again after the swap.
pub(crate) const RELAUNCHES_AFTER_SWAP: bool = false;
/// A running executable cannot be replaced, only renamed: it is moved aside first.
pub(crate) const SWAPS_IN_ONE_RENAME: bool = false;

/// The release asset for a Windows machine of the given architecture
/// (`std::env::consts::ARCH`): only x86-64 is released.
pub fn asset_name_for_arch(arch: &str) -> Option<&'static str> {
    (arch == "x86_64").then_some("atrium-windows-x86_64.exe")
}

/// Where the running executable is installed; never self-updatable yet.
pub fn install_location(debug_build: bool) -> InstallLocation {
    match std::env::current_exe() {
        Ok(executable) => resolve(&executable, debug_build, &can_replace),
        Err(error) => InstallLocation::ManualOnly {
            reason: format!("cannot locate Atrium: {error}"),
        },
    }
}

/// Why `executable` does not update itself: a development build, a folder the user may not
/// write to (`can_replace`), or else because Windows self-update is still missing.
fn resolve(
    executable: &Path,
    debug_build: bool,
    can_replace: &dyn Fn(&Path) -> bool,
) -> InstallLocation {
    let reason = if debug_build {
        "this is a development build"
    } else if !can_replace(executable) {
        "the folder of Atrium is not writable for you"
    } else {
        "Atrium does not install updates itself on Windows yet"
    };
    InstallLocation::ManualOnly {
        reason: reason.into(),
    }
}

/// An executable is not an `AppImage`.
#[must_use]
pub fn running_appimage() -> Option<RunningAppImage> {
    None
}

/// Why a release without the file for this machine is only announced.
pub(crate) const NO_DOWNLOAD_REASON: &str = "the release has no download for this computer";

/// The sentence the About tab shows when updates have to be installed by hand.
pub(crate) fn manual_only_text(reason: &str) -> String {
    format!("Install updates by hand: {reason}.")
}

/// Moves the verified download next to the install (`<install>.new-update`), which it returns.
/// Identity and version come from `SHA256SUMS` and the tag.
pub(crate) fn prepare_update(
    downloaded: &Path,
    _version: Version,
    install: &Path,
) -> Result<PathBuf, String> {
    place_next_to_install(downloaded, install)
}

/// The helper is the installed executable.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the platforms share this signature; macOS can fail"
)]
pub(crate) fn helper_executable(install: &Path) -> std::io::Result<PathBuf> {
    Ok(install.to_path_buf())
}

/// Every process counts as alive, so no swap ever starts under a running Atrium.
pub(crate) fn process_alive(_pid: u32) -> bool {
    true
}

/// Always empty: nothing is swapped on Windows yet.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the platforms share this signature; macOS can fail"
)]
pub(crate) fn processes_running_from(_install: &Path) -> Result<Vec<u32>, String> {
    Ok(Vec::new())
}

/// Nothing to do: the user opens Atrium again.
pub(crate) fn relaunch(_install: &Path) {}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::swap::new_update_path;

    /// The release asset the flow tests use.
    pub(crate) const ASSET: &str = "atrium-windows-x86_64.exe";

    /// The release asset of Atrium in `version`: a file holding the version.
    pub(crate) fn fake_asset(_dir: &Path, version: &str) -> Vec<u8> {
        version.as_bytes().to_vec()
    }

    /// `atrium.exe` in version 0.1.0 in `<dir>/Atrium`; returns its path.
    pub(crate) fn fake_install(dir: &Path) -> PathBuf {
        let folder = dir.join("Atrium");
        std::fs::create_dir_all(&folder).unwrap();
        let install = folder.join("atrium.exe");
        std::fs::write(&install, fake_asset(dir, "0.1.0")).unwrap();
        install
    }

    /// Stages the executable of `version` next to `install`.
    pub(crate) fn fake_staged(dir: &Path, install: &Path, version: &str) {
        std::fs::write(new_update_path(install), fake_asset(dir, version)).unwrap();
    }

    /// Fails unless `path` is the executable of `version`.
    pub(crate) fn verify_staged(path: &Path, version: &str) {
        assert_eq!(std::fs::read(path).unwrap(), fake_asset(path, version));
    }

    #[test]
    fn only_x86_64_has_an_asset() {
        assert_eq!(
            asset_name_for_arch("x86_64"),
            Some("atrium-windows-x86_64.exe")
        );
        assert_eq!(asset_name_for_arch("aarch64"), None);
    }

    #[test]
    fn every_installation_only_notifies() {
        let install = Path::new(r"C:\Users\u\Atrium\atrium.exe");
        let cases = [
            (true, true, "development"),
            (false, false, "not writable"),
            (false, true, "not install updates itself on Windows yet"),
        ];
        for (debug, replaceable, expected) in cases {
            let location = resolve(install, debug, &|_| replaceable);
            assert!(
                location
                    .manual_reason()
                    .unwrap_or_default()
                    .contains(expected),
                "{location:?}"
            );
        }
    }
}
