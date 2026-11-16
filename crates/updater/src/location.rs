//! Where Atrium runs from, and whether that installation may replace itself.

use std::fs::OpenOptions;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallLocation {
    /// `bundle` is the `Atrium.app` that gets swapped after Atrium quit.
    SelfUpdatable { bundle: PathBuf },
    /// Updates are only announced; the user installs them from the release page.
    ManualOnly { reason: String },
}

/// Whether the user can create files next to `bundle` (needed for the renames) and owns it
/// (needed to remove the old version). The probe file never goes inside the bundle.
pub fn can_replace(bundle: &Path) -> bool {
    let Some(parent) = bundle.parent() else {
        return false;
    };
    let probe = parent.join(format!(".atrium-update-probe-{}", std::process::id()));
    let Ok(file) = OpenOptions::new().write(true).create_new(true).open(&probe) else {
        return false;
    };
    let own_uid = file.metadata().map(|meta| meta.uid());
    let _ = std::fs::remove_file(&probe);
    let bundle_uid = std::fs::metadata(bundle).map(|meta| meta.uid());
    matches!((own_uid, bundle_uid), (Ok(own), Ok(owner)) if own == owner)
}

impl InstallLocation {
    /// Why updates are only announced; None for an installation that replaces itself.
    pub fn manual_reason(&self) -> Option<&str> {
        match self {
            InstallLocation::ManualOnly { reason } => Some(reason),
            InstallLocation::SelfUpdatable { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_bundle_in_a_writable_folder_can_be_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Atrium.app");
        std::fs::create_dir(&bundle).unwrap();
        assert!(can_replace(&bundle));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(!can_replace(&dir.path().join("missing/Atrium.app")));
    }
}
