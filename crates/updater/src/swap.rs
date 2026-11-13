//! Replacing the installed bundle with the staged one: two renames on one volume, with a
//! rollback when the second fails. Runs only after Atrium exited (see `helper`).

use std::io;
use std::path::{Path, PathBuf};

/// The file operations of a swap, so the failure paths can be tested.
pub(crate) trait BundleFs {
    fn exists(&self, path: &Path) -> bool;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_all(&self, path: &Path) -> io::Result<()>;
}

/// The real file system.
pub(crate) struct RealFs;

impl BundleFs for RealFs {
    fn exists(&self, path: &Path) -> bool {
        path.symlink_metadata().is_ok()
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove_all(&self, path: &Path) -> io::Result<()> {
        if path.symlink_metadata()?.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        }
    }
}

/// `<install>.new-update`: the verified update, next to the install so the swap is a rename.
pub(crate) fn new_update_path(install: &Path) -> PathBuf {
    sibling_with_suffix(install, ".new-update")
}

/// `<install>.new-update.partial`: a cross-volume copy of the update until it is complete.
pub(crate) fn partial_update_path(install: &Path) -> PathBuf {
    sibling_with_suffix(install, ".new-update.partial")
}

/// `<install>.old-update`: the previous version during the swap.
pub(crate) fn old_update_path(install: &Path) -> PathBuf {
    sibling_with_suffix(install, ".old-update")
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SwapOutcome {
    Installed,
    /// The install was not touched.
    NotStarted(String),
    /// The update could not be moved in place; the previous version is back.
    RolledBack(String),
    /// Neither version is in place; the previous one is at `<install>.old-update`.
    RollbackFailed(String),
}

impl SwapOutcome {
    pub(crate) fn message(&self) -> String {
        match self {
            SwapOutcome::Installed => "updated".into(),
            SwapOutcome::NotStarted(reason) => reason.clone(),
            SwapOutcome::RolledBack(reason) => {
                format!("{reason}; the previous version was restored")
            }
            SwapOutcome::RollbackFailed(reason) => format!(
                "{reason}; restoring failed, the previous version is at <install>.old-update"
            ),
        }
    }
}

/// Moves `install` aside, the staged update in its place, then removes the old version
/// (a leftover is removed on the next start).
pub(crate) fn swap(install: &Path, fs: &dyn BundleFs) -> SwapOutcome {
    let staged = new_update_path(install);
    let backup = old_update_path(install);
    if !fs.exists(&staged) {
        return SwapOutcome::NotStarted("the staged update is missing".into());
    }
    if fs.exists(&backup)
        && let Err(error) = fs.remove_all(&backup)
    {
        return SwapOutcome::NotStarted(format!("cannot remove an old backup: {error}"));
    }
    if let Err(error) = fs.rename(install, &backup) {
        return SwapOutcome::NotStarted(format!("cannot move the current version aside: {error}"));
    }
    match fs.rename(&staged, install) {
        Ok(()) => {
            let _ = fs.remove_all(&backup);
            SwapOutcome::Installed
        }
        Err(error) => roll_back(
            install,
            &backup,
            fs,
            format!("cannot move the update in place: {error}"),
        ),
    }
}

fn roll_back(install: &Path, backup: &Path, fs: &dyn BundleFs, reason: String) -> SwapOutcome {
    if fs.exists(install) {
        let _ = fs.remove_all(install);
    }
    match fs.rename(backup, install) {
        Ok(()) => SwapOutcome::RolledBack(reason),
        Err(_) => SwapOutcome::RollbackFailed(reason),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// The real file system, with the listed renames (counted from 1) failing.
    struct FailingRename {
        failing: &'static [usize],
        renames: Cell<usize>,
    }

    impl FailingRename {
        fn new(failing: &'static [usize]) -> FailingRename {
            FailingRename {
                failing,
                renames: Cell::new(0),
            }
        }
    }

    impl BundleFs for FailingRename {
        fn exists(&self, path: &Path) -> bool {
            RealFs.exists(path)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.renames.set(self.renames.get() + 1);
            if self.failing.contains(&self.renames.get()) {
                return Err(io::Error::other("injected"));
            }
            RealFs.rename(from, to)
        }

        fn remove_all(&self, path: &Path) -> io::Result<()> {
            RealFs.remove_all(path)
        }
    }

    /// `Atrium.app` with version file "old" and a staged update with "new".
    fn install(dir: &Path) -> PathBuf {
        let install = dir.join("Atrium.app");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("version"), "old").unwrap();
        let staged = new_update_path(&install);
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("version"), "new").unwrap();
        install
    }

    fn installed_version(install: &Path) -> String {
        std::fs::read_to_string(install.join("version")).unwrap()
    }

    #[test]
    fn paths_are_siblings_of_the_install() {
        let install = Path::new("/Applications/Atrium.app");
        assert_eq!(
            new_update_path(install),
            Path::new("/Applications/Atrium.app.new-update")
        );
        assert_eq!(
            old_update_path(install),
            Path::new("/Applications/Atrium.app.old-update")
        );
    }

    #[test]
    fn swaps_and_removes_the_old_version() {
        let dir = tempfile::tempdir().unwrap();
        let install = install(dir.path());
        std::fs::create_dir(old_update_path(&install)).unwrap();
        assert_eq!(swap(&install, &RealFs), SwapOutcome::Installed);
        assert_eq!(installed_version(&install), "new");
        assert!(!new_update_path(&install).exists());
        assert!(!old_update_path(&install).exists());
    }

    #[test]
    fn missing_update_leaves_the_install_alone() {
        let dir = tempfile::tempdir().unwrap();
        let install = install(dir.path());
        std::fs::remove_dir_all(new_update_path(&install)).unwrap();
        assert!(matches!(
            swap(&install, &RealFs),
            SwapOutcome::NotStarted(_)
        ));
        assert_eq!(installed_version(&install), "old");
    }

    #[test]
    fn failing_first_rename_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let install = install(dir.path());
        assert!(matches!(
            swap(&install, &FailingRename::new(&[1])),
            SwapOutcome::NotStarted(_)
        ));
        assert_eq!(installed_version(&install), "old");
        assert!(new_update_path(&install).exists());
    }

    #[test]
    fn failing_second_rename_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let install = install(dir.path());
        let outcome = swap(&install, &FailingRename::new(&[2]));
        assert!(matches!(outcome, SwapOutcome::RolledBack(_)), "{outcome:?}");
        assert!(outcome.message().contains("restored"));
        assert_eq!(installed_version(&install), "old");
        assert!(!old_update_path(&install).exists());
    }

    #[test]
    fn failing_rollback_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let install = install(dir.path());
        let outcome = swap(&install, &FailingRename::new(&[2, 3]));
        assert!(
            matches!(outcome, SwapOutcome::RollbackFailed(_)),
            "{outcome:?}"
        );
        assert!(old_update_path(&install).exists());
    }
}
