//! Linux: no self-update yet. The app does not start the updater on Linux; these stand-ins only
//! let the shared code build there.

use std::path::{Path, PathBuf};

use crate::location::InstallLocation;
use crate::version::Version;

/// No release asset for Linux yet.
pub fn asset_name_for_arch(_arch: &str) -> Option<&'static str> {
    None
}

/// Never self-updatable.
pub fn install_location(_debug_build: bool) -> InstallLocation {
    InstallLocation::ManualOnly {
        reason: "Atrium does not update itself on Linux yet".into(),
    }
}

/// Why a release without the file for this machine is only announced.
pub(crate) const NO_DOWNLOAD_REASON: &str = "the release has no download for this computer";

/// The sentence the About tab shows when updates have to be installed by hand.
pub(crate) fn manual_only_text(reason: &str) -> String {
    format!("Install updates by hand: {reason}.")
}

/// Nothing is staged on Linux.
pub(crate) fn prepare_update(
    _downloaded: &Path,
    _version: Version,
    _install: &Path,
) -> Result<PathBuf, String> {
    Err("Atrium cannot install updates on Linux yet".into())
}

/// Every process counts as alive, so no swap ever starts under a running Atrium.
pub(crate) fn process_alive(_pid: u32) -> bool {
    true
}

/// Always empty: nothing is swapped on Linux.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the platforms share this signature; macOS can fail"
)]
pub(crate) fn processes_running_from(_install: &Path) -> Result<Vec<u32>, String> {
    Ok(Vec::new())
}

/// Nothing to do: nothing is swapped on Linux.
pub(crate) fn relaunch(_install: &Path) {}
