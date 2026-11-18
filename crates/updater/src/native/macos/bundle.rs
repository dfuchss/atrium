//! Checks that an extracted app bundle is the Atrium release that was selected.

use std::path::{Path, PathBuf};

use crate::version::Version;

/// `identifier` in `app/src-tauri/tauri.conf.json`; a bundle with another id is refused.
pub(crate) const BUNDLE_IDENTIFIER: &str = "org.fuchss.atrium";

/// The `Info.plist` keys the check needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct BundleInfo {
    pub(crate) identifier: Option<String>,
    pub(crate) short_version: Option<String>,
    pub(crate) executable: Option<String>,
}

impl BundleInfo {
    /// Reads `<bundle>/Contents/Info.plist`.
    pub(crate) fn read(bundle: &Path) -> Result<BundleInfo, String> {
        let plist = plist::Value::from_file(bundle.join("Contents/Info.plist"))
            .map_err(|error| format!("cannot read Info.plist: {error}"))?;
        let dictionary = plist.as_dictionary().ok_or("Info.plist is no dictionary")?;
        let text = |key: &str| {
            dictionary
                .get(key)
                .and_then(plist::Value::as_string)
                .map(String::from)
        };
        Ok(BundleInfo {
            identifier: text("CFBundleIdentifier"),
            short_version: text("CFBundleShortVersionString"),
            executable: text("CFBundleExecutable"),
        })
    }

    /// The bundle is Atrium in exactly `expected` and names a plain executable file name.
    pub(crate) fn check(&self, expected: Version) -> Result<(), String> {
        if self.identifier.as_deref() != Some(BUNDLE_IDENTIFIER) {
            return Err(format!(
                "the update has bundle identifier {:?}, expected {BUNDLE_IDENTIFIER}",
                self.identifier
            ));
        }
        let version = self.short_version.as_deref().and_then(Version::parse);
        if version != Some(expected) {
            return Err(format!(
                "the update has version {:?}, expected {expected}",
                self.short_version
            ));
        }
        match self.executable.as_deref() {
            Some(name) if !name.is_empty() && !name.contains('/') && name != ".." => Ok(()),
            other => Err(format!("the update names no valid executable: {other:?}")),
        }
    }
}

/// The only `*.app` folder directly inside `extracted`.
pub(crate) fn find_bundle(extracted: &Path) -> Result<PathBuf, String> {
    let entries = std::fs::read_dir(extracted)
        .map_err(|error| format!("cannot list {}: {error}", extracted.display()))?;
    let bundles: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
        .filter(|path| path.symlink_metadata().is_ok_and(|meta| meta.is_dir()))
        .collect();
    match bundles.as_slice() {
        [bundle] => Ok(bundle.clone()),
        [] => Err("the archive contains no app".into()),
        _ => Err("the archive contains more than one app".into()),
    }
}

/// Reads and checks the bundle, and that its executable is a regular file.
pub(crate) fn verify(bundle: &Path, expected: Version) -> Result<(), String> {
    let info = BundleInfo::read(bundle)?;
    info.check(expected)?;
    let executable = bundle
        .join("Contents/MacOS")
        .join(info.executable.unwrap_or_default());
    if executable
        .symlink_metadata()
        .is_ok_and(|meta| meta.is_file())
    {
        Ok(())
    } else {
        Err("the update's executable is missing".into())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Writes a minimal `Atrium.app` into `dir` and returns its path.
    pub(crate) fn fake_bundle(dir: &Path, identifier: &str, version: &str) -> PathBuf {
        let bundle = dir.join("Atrium.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        std::fs::write(bundle.join("Contents/MacOS/atrium"), "binary").unwrap();
        let mut info = plist::Dictionary::new();
        info.insert("CFBundleIdentifier".into(), identifier.into());
        info.insert("CFBundleShortVersionString".into(), version.into());
        info.insert("CFBundleExecutable".into(), "atrium".into());
        plist::Value::Dictionary(info)
            .to_file_xml(bundle.join("Contents/Info.plist"))
            .unwrap();
        bundle
    }

    fn info(identifier: &str, version: &str, executable: &str) -> BundleInfo {
        BundleInfo {
            identifier: Some(identifier.into()),
            short_version: Some(version.into()),
            executable: Some(executable.into()),
        }
    }

    fn v020() -> Version {
        Version::parse("0.2.0").unwrap()
    }

    #[test]
    fn accepts_atrium_in_the_selected_version() {
        info(BUNDLE_IDENTIFIER, "0.2.0", "atrium")
            .check(v020())
            .unwrap();
    }

    #[test]
    fn refuses_other_apps_versions_and_executables() {
        let refused = [
            info("org.example.other", "0.2.0", "atrium"),
            info(BUNDLE_IDENTIFIER, "0.1.0", "atrium"),
            info(BUNDLE_IDENTIFIER, "0.2.0-rc.1", "atrium"),
            info(BUNDLE_IDENTIFIER, "0.2.0", "../../bin/sh"),
            info(BUNDLE_IDENTIFIER, "0.2.0", ""),
            BundleInfo::default(),
        ];
        for bundle_info in refused {
            assert!(bundle_info.check(v020()).is_err(), "{bundle_info:?}");
        }
    }

    #[test]
    fn verifies_a_bundle_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = fake_bundle(dir.path(), BUNDLE_IDENTIFIER, "0.2.0");
        assert_eq!(find_bundle(dir.path()).unwrap(), bundle);
        verify(&bundle, v020()).unwrap();
        std::fs::remove_file(bundle.join("Contents/MacOS/atrium")).unwrap();
        assert!(verify(&bundle, v020()).is_err());
    }

    #[test]
    fn finds_exactly_one_bundle() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_bundle(dir.path()).is_err());
        std::fs::create_dir(dir.path().join("One.app")).unwrap();
        std::fs::create_dir(dir.path().join("Two.app")).unwrap();
        assert!(find_bundle(dir.path()).is_err());
    }
}
