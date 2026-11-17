//! Replacing the installation with the staged update on one volume: a file in one atomic
//! rename, a folder (or a running Windows executable) in two renames with a rollback when the
//! second fails. Runs only after
//! Atrium exited (see `helper`).

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

/// Puts the staged update in place of `install`: `in_one_rename` (a file) renames it over the
/// install, so either version is always there and running copies keep the old file; otherwise
/// (a folder, or a running Windows executable, which a rename cannot replace) see
/// [`move_aside_and_swap`].
pub(crate) fn swap(install: &Path, fs: &dyn BundleFs, in_one_rename: bool) -> SwapOutcome {
    let staged = new_update_path(install);
    if !fs.exists(&staged) {
        return SwapOutcome::NotStarted("the staged update is missing".into());
    }
    if !in_one_rename {
        return move_aside_and_swap(install, &staged, fs);
    }
    match fs.rename(&staged, install) {
        Ok(()) => SwapOutcome::Installed,
        Err(error) => SwapOutcome::NotStarted(format!("cannot move the update in place: {error}")),
    }
}

/// Moves `install` aside, the staged update in its place, then removes the old version
/// (a leftover, such as the executable the Windows helper runs from, is removed on the next
/// start).
fn move_aside_and_swap(install: &Path, staged: &Path, fs: &dyn BundleFs) -> SwapOutcome {
    let backup = old_update_path(install);
    if fs.exists(&backup)
        && let Err(error) = fs.remove_all(&backup)
    {
        return SwapOutcome::NotStarted(format!("cannot remove an old backup: {error}"));
    }
    if let Err(error) = fs.rename(install, &backup) {
        return SwapOutcome::NotStarted(format!("cannot move the current version aside: {error}"));
    }
    match fs.rename(staged, install) {
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

    /// The `in_one_rename` of a file (Linux) and of a folder (macOS).
    const ONE_RENAME: bool = true;
    const TWO_RENAMES: bool = false;

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
        assert_eq!(swap(&install, &RealFs, TWO_RENAMES), SwapOutcome::Installed);
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
            swap(&install, &RealFs, TWO_RENAMES),
            SwapOutcome::NotStarted(_)
        ));
        assert_eq!(installed_version(&install), "old");
    }

    #[test]
    fn failing_first_rename_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let install = install(dir.path());
        assert!(matches!(
            swap(&install, &FailingRename::new(&[1]), TWO_RENAMES),
            SwapOutcome::NotStarted(_)
        ));
        assert_eq!(installed_version(&install), "old");
        assert!(new_update_path(&install).exists());
    }

    #[test]
    fn failing_second_rename_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let install = install(dir.path());
        let outcome = swap(&install, &FailingRename::new(&[2]), TWO_RENAMES);
        assert!(matches!(outcome, SwapOutcome::RolledBack(_)), "{outcome:?}");
        assert!(outcome.message().contains("restored"));
        assert_eq!(installed_version(&install), "old");
        assert!(!old_update_path(&install).exists());
    }

    #[test]
    fn failing_rollback_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let install = install(dir.path());
        let outcome = swap(&install, &FailingRename::new(&[2, 3]), TWO_RENAMES);
        assert!(
            matches!(outcome, SwapOutcome::RollbackFailed(_)),
            "{outcome:?}"
        );
        assert!(old_update_path(&install).exists());
    }

    /// `Atrium.AppImage` with "old" and a staged update with "new".
    fn file_install(dir: &Path) -> PathBuf {
        let install = dir.join("Atrium.AppImage");
        std::fs::write(&install, "old").unwrap();
        std::fs::write(new_update_path(&install), "new").unwrap();
        install
    }

    #[test]
    fn replaces_a_file_in_one_rename_and_keeps_open_copies() {
        let dir = tempfile::tempdir().unwrap();
        let install = file_install(dir.path());
        let running = std::fs::File::open(&install).unwrap();
        assert_eq!(swap(&install, &RealFs, ONE_RENAME), SwapOutcome::Installed);
        assert_eq!(std::fs::read_to_string(&install).unwrap(), "new");
        assert!(!new_update_path(&install).exists());
        assert!(!old_update_path(&install).exists());
        assert_eq!(std::io::read_to_string(running).unwrap(), "old");
    }

    #[test]
    fn a_failing_rename_leaves_the_file_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let install = file_install(dir.path());
        let outcome = swap(&install, &FailingRename::new(&[1]), ONE_RENAME);
        assert!(matches!(outcome, SwapOutcome::NotStarted(_)), "{outcome:?}");
        assert_eq!(std::fs::read_to_string(&install).unwrap(), "old");
        assert!(new_update_path(&install).exists());
        std::fs::remove_file(new_update_path(&install)).unwrap();
        assert!(matches!(
            swap(&install, &RealFs, ONE_RENAME),
            SwapOutcome::NotStarted(_)
        ));
    }
}
