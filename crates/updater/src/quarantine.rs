//! Removes the quarantine attribute from a verified update bundle.
//!
//! Atrium sets `LSFileQuarantineEnabled`, so everything it downloads or extracts (also through
//! `ditto`) carries `com.apple.quarantine`. An unsigned, quarantined bundle is blocked by
//! Gatekeeper or translocated when the helper opens it, so the update would never start.

use std::path::Path;

const QUARANTINE_ATTRIBUTE: &str = "com.apple.quarantine";

/// Removes `com.apple.quarantine` from `root` and everything below it. Symbolic links are
/// cleared themselves and never followed.
pub(crate) fn remove_quarantine(root: &Path) -> Result<(), String> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        clear_attribute(&path)?;
        let is_dir = path
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?
            .is_dir();
        if is_dir {
            let entries = std::fs::read_dir(&path)
                .map_err(|error| format!("cannot list {}: {error}", path.display()))?;
            for entry in entries {
                pending.push(entry.map_err(|error| error.to_string())?.path());
            }
        }
    }
    Ok(())
}

fn clear_attribute(path: &Path) -> Result<(), String> {
    let failed = |error: std::io::Error| {
        format!(
            "cannot remove the quarantine of {}: {error}",
            path.display()
        )
    };
    // `get` and `remove` do not follow symbolic links.
    if xattr::get(path, QUARANTINE_ATTRIBUTE)
        .map_err(failed)?
        .is_some()
    {
        xattr::remove(path, QUARANTINE_ATTRIBUTE).map_err(failed)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALUE: &[u8] = b"0083;00000000;Atrium;";

    #[test]
    fn clears_the_whole_tree_without_following_links() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::write(&outside, "keep").unwrap();
        xattr::set(&outside, QUARANTINE_ATTRIBUTE, VALUE).unwrap();
        let bundle = dir.path().join("Atrium.app");
        let macos = bundle.join("Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::write(macos.join("atrium"), "binary").unwrap();
        std::os::unix::fs::symlink(&outside, bundle.join("Contents/link")).unwrap();
        std::os::unix::fs::symlink("missing", bundle.join("dangling")).unwrap();
        let marked = [
            bundle.clone(),
            bundle.join("Contents"),
            macos.join("atrium"),
        ];
        for path in &marked {
            xattr::set(path, QUARANTINE_ATTRIBUTE, VALUE).unwrap();
        }

        remove_quarantine(&bundle).unwrap();

        for path in &marked {
            assert_eq!(xattr::get(path, QUARANTINE_ATTRIBUTE).unwrap(), None);
        }
        let kept = xattr::get(&outside, QUARANTINE_ATTRIBUTE).unwrap();
        assert_eq!(kept.as_deref(), Some(VALUE));
    }

    #[test]
    fn a_missing_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(remove_quarantine(&dir.path().join("missing")).is_err());
    }
}
