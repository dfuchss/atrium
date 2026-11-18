//! Where Atrium runs from, and whether that installation may replace itself.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallLocation {
    /// `bundle` is the installation (`Atrium.app`, the `AppImage` on Linux, the executable on
    /// Windows) that gets swapped after Atrium quit.
    SelfUpdatable { bundle: PathBuf },
    /// Updates are only announced; the user installs them from the release page.
    ManualOnly { reason: String },
}

/// Whether the user can create files next to `bundle` (needed for the renames) and owns it
/// (needed to remove the old version). The probe file never goes inside the bundle.
pub(crate) fn can_replace(bundle: &Path) -> bool {
    let Some(parent) = bundle.parent() else {
        return false;
    };
    let probe = parent.join(format!(".atrium-update-probe-{}", std::process::id()));
    let Ok(file) = OpenOptions::new().write(true).create_new(true).open(&probe) else {
        return false;
    };
    let owned = owns(&file, bundle);
    drop(file);
    let _ = std::fs::remove_file(&probe);
    owned
}

/// Whether the owner of `probe`, a file the user just created, owns `bundle`.
#[cfg(unix)]
fn owns(probe: &File, bundle: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    let own_uid = probe.metadata().map(|meta| meta.uid());
    let bundle_uid = std::fs::metadata(bundle).map(|meta| meta.uid());
    matches!((own_uid, bundle_uid), (Ok(own), Ok(owner)) if own == owner)
}

/// Whether `bundle` exists: Windows has no owner ids to compare, so the probe file the user
/// could create next to it has to do.
#[cfg(not(unix))]
fn owns(_probe: &File, bundle: &Path) -> bool {
    bundle.exists()
}

/// Where the running installation is: `env` is what the platform found out about the running
/// program, `resolve` decides from it whether it may replace itself (given `can_replace`) and
/// returns the installation, or why it only announces updates.
pub(crate) fn locate<E>(
    debug_build: bool,
    env: std::io::Result<E>,
    resolve: impl FnOnce(&E, &dyn Fn(&Path) -> bool) -> Result<PathBuf, String>,
) -> InstallLocation {
    let manual = |reason: String| InstallLocation::ManualOnly { reason };
    match env {
        Err(error) => manual(format!("cannot locate Atrium: {error}")),
        Ok(_) if debug_build => manual("this is a development build".into()),
        Ok(env) => match resolve(&env, &can_replace) {
            Ok(bundle) => InstallLocation::SelfUpdatable { bundle },
            Err(reason) => manual(reason),
        },
    }
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

    #[test]
    fn locating_stops_before_the_platform_for_a_missing_executable_or_a_development_build() {
        let unreachable = |(): &(), _: &dyn Fn(&Path) -> bool| -> Result<PathBuf, String> {
            panic!("the platform must not be asked")
        };
        let failed = locate(false, Err(std::io::Error::other("gone")), unreachable);
        assert_eq!(failed.manual_reason(), Some("cannot locate Atrium: gone"));
        let development = locate(true, Ok(()), unreachable);
        assert_eq!(
            development.manual_reason(),
            Some("this is a development build")
        );
    }

    #[test]
    fn locating_reports_what_the_platform_decides() {
        let bundle = PathBuf::from("/Applications/Atrium.app");
        assert_eq!(
            locate(false, Ok(()), |(), _| Ok(bundle.clone())),
            InstallLocation::SelfUpdatable { bundle }
        );
        let manual = locate(false, Ok(()), |(), _| Err("not here".into()));
        assert_eq!(manual.manual_reason(), Some("not here"));
    }
}
