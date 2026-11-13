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

impl InstallLocation {
    /// Decides for the running `executable` (`<bundle>/Contents/MacOS/<name>`). Only bundles
    /// directly in `/Applications` or `~/Applications` update themselves, and only when
    /// `can_replace(bundle)` says the user may rename and remove it.
    pub fn resolve(
        executable: &Path,
        home: Option<&Path>,
        debug_build: bool,
        can_replace: &dyn Fn(&Path) -> bool,
    ) -> InstallLocation {
        let manual = |reason: &str| InstallLocation::ManualOnly {
            reason: reason.to_string(),
        };
        if debug_build {
            return manual("this is a development build");
        }
        let Some(bundle) = bundle_of(executable) else {
            return manual("Atrium is not running from an app bundle");
        };
        if bundle.to_string_lossy().contains("/AppTranslocation/") {
            return manual("macOS runs Atrium from a temporary location; move it to Applications");
        }
        let in_applications = bundle.parent().is_some_and(|parent| {
            parent == Path::new("/Applications")
                || home.is_some_and(|home| parent == home.join("Applications"))
        });
        if !in_applications {
            return manual("Atrium is not installed in the Applications folder");
        }
        if !can_replace(&bundle) {
            return manual("the Applications folder or Atrium.app is not writable for you");
        }
        InstallLocation::SelfUpdatable { bundle }
    }
}

/// `<bundle>.app` of `<bundle>.app/Contents/MacOS/<executable>`.
fn bundle_of(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    is_bundle.then(|| bundle.to_path_buf())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(executable: &str, debug: bool, replaceable: bool) -> InstallLocation {
        InstallLocation::resolve(
            Path::new(executable),
            Some(Path::new("/Users/test")),
            debug,
            &|_| replaceable,
        )
    }

    fn reason(location: &InstallLocation) -> &str {
        match location {
            InstallLocation::ManualOnly { reason } => reason,
            InstallLocation::SelfUpdatable { .. } => "",
        }
    }

    #[test]
    fn bundles_in_applications_update_themselves() {
        for (executable, bundle) in [
            (
                "/Applications/Atrium.app/Contents/MacOS/atrium",
                "/Applications/Atrium.app",
            ),
            (
                "/Users/test/Applications/Atrium.app/Contents/MacOS/Atrium",
                "/Users/test/Applications/Atrium.app",
            ),
        ] {
            assert_eq!(
                resolve(executable, false, true),
                InstallLocation::SelfUpdatable {
                    bundle: bundle.into()
                }
            );
        }
    }

    #[test]
    fn everything_else_only_notifies() {
        let cases = [
            (
                "/Applications/Atrium.app/Contents/MacOS/atrium",
                true,
                true,
                "development",
            ),
            (
                "/Users/test/src/atrium/target/release/atrium",
                false,
                true,
                "app bundle",
            ),
            (
                "/private/var/folders/x/AppTranslocation/1/d/Atrium.app/Contents/MacOS/atrium",
                false,
                true,
                "temporary",
            ),
            (
                "/Users/test/Downloads/Atrium.app/Contents/MacOS/atrium",
                false,
                true,
                "Applications",
            ),
            (
                "/Volumes/Atrium/Atrium.app/Contents/MacOS/atrium",
                false,
                true,
                "Applications",
            ),
            (
                "/Applications/Tools/Atrium.app/Contents/MacOS/atrium",
                false,
                true,
                "Applications",
            ),
            (
                "/Applications/Atrium.app/Contents/MacOS/atrium",
                false,
                false,
                "writable",
            ),
        ];
        for (executable, debug, replaceable, expected) in cases {
            let location = resolve(executable, debug, replaceable);
            assert!(
                reason(&location).contains(expected),
                "{executable}: {location:?}"
            );
        }
    }

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
