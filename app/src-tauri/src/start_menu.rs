//! The Start menu shortcut that Atrium installs for itself on Windows: `Atrium.lnk` in the
//! user's Start menu programs folder (`%APPDATA%\Microsoft\Windows\Start Menu\Programs`). It
//! starts the running executable under Atrium's identifier as application user model id, so
//! Windows names Atrium's notifications and taskbar button after it. Uninstall removes the
//! shortcut only.

use std::path::{Path, PathBuf};

use protocol::DesktopEntryState;
use webview_dialogs::{read_shortcut, write_shortcut};

const SHORTCUT_FILE: &str = "Atrium.lnk";
/// The Start menu programs folder of the user, below the roaming app data folder.
const PROGRAMS: &str = r"Microsoft\Windows\Start Menu\Programs";

/// The folder the shortcut goes to.
pub(crate) struct StartMenu {
    programs: PathBuf,
}

impl StartMenu {
    /// The Start menu of the user whose roaming app data folder is `roaming`.
    pub(crate) fn in_roaming(roaming: &Path) -> StartMenu {
        StartMenu {
            programs: roaming.join(PROGRAMS),
        }
    }

    fn shortcut_path(&self) -> PathBuf {
        self.programs.join(SHORTCUT_FILE)
    }
}

/// Whether the shortcut exists and starts `executable`.
pub(crate) fn state(menu: &StartMenu, executable: &Path) -> Result<DesktopEntryState, String> {
    let link = menu.shortcut_path();
    if !link.exists() {
        return Ok(DesktopEntryState::NotInstalled);
    }
    let shortcut =
        read_shortcut(&link).map_err(|error| format!("cannot read {}: {error}", link.display()))?;
    Ok(if same_file(&shortcut.target, executable) {
        DesktopEntryState::Installed
    } else {
        DesktopEntryState::OtherPath {
            path: shortcut.target.display().to_string(),
        }
    })
}

/// Whether `a` and `b` name the same file. Windows keeps the long form of a shortcut's target,
/// while the executable may come as a short (8.3) path or in other letter case.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(a), Ok(b)) if a == b
        )
}

/// Writes or rewrites the shortcut that starts `executable` as `app_id`.
pub(crate) fn install(menu: &StartMenu, executable: &Path, app_id: &str) -> Result<(), String> {
    let link = menu.shortcut_path();
    std::fs::create_dir_all(&menu.programs)
        .map_err(|error| format!("cannot create {}: {error}", menu.programs.display()))?;
    write_shortcut(&link, executable, app_id)
        .map_err(|error| format!("cannot write {}: {error}", link.display()))
}

/// Removes the shortcut, if there is one.
pub(crate) fn uninstall(menu: &StartMenu) -> Result<(), String> {
    let link = menu.shortcut_path();
    fs_util::remove_if_present(&link)
        .map_err(|error| format!("cannot remove {}: {error}", link.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Start menu below `root` and an executable there.
    fn menu_and_executable(root: &Path, name: &str) -> (StartMenu, PathBuf) {
        let executable = root.join("Programs").join(name);
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, "exe").unwrap();
        (StartMenu::in_roaming(&root.join("Roaming")), executable)
    }

    #[test]
    fn install_points_the_shortcut_at_the_executable() {
        let root = tempfile::tempdir().unwrap();
        let (menu, executable) = menu_and_executable(root.path(), "Atrium.exe");
        assert_eq!(
            state(&menu, &executable).unwrap(),
            DesktopEntryState::NotInstalled
        );
        install(&menu, &executable, "org.fuchss.atrium.dev").unwrap();
        assert!(
            root.path()
                .join(r"Roaming\Microsoft\Windows\Start Menu\Programs\Atrium.lnk")
                .is_file()
        );
        assert_eq!(
            state(&menu, &executable).unwrap(),
            DesktopEntryState::Installed
        );
        let shortcut = read_shortcut(&menu.shortcut_path()).unwrap();
        assert_eq!(shortcut.app_id.as_deref(), Some("org.fuchss.atrium.dev"));
    }

    #[test]
    fn a_moved_executable_is_another_path_until_installed_again() {
        let root = tempfile::tempdir().unwrap();
        let (menu, executable) = menu_and_executable(root.path(), "Atrium.exe");
        install(&menu, &executable, "org.fuchss.atrium.dev").unwrap();
        let (_, moved) = menu_and_executable(root.path(), "Moved.exe");
        match state(&menu, &moved).unwrap() {
            DesktopEntryState::OtherPath { path } => {
                assert!(same_file(Path::new(&path), &executable), "{path}");
            }
            other => panic!("expected another path, got {other:?}"),
        }
        install(&menu, &moved, "org.fuchss.atrium.dev").unwrap();
        assert_eq!(state(&menu, &moved).unwrap(), DesktopEntryState::Installed);
    }

    #[test]
    fn uninstall_removes_only_the_shortcut() {
        let root = tempfile::tempdir().unwrap();
        let (menu, executable) = menu_and_executable(root.path(), "Atrium.exe");
        install(&menu, &executable, "org.fuchss.atrium.dev").unwrap();
        let other = menu.programs.join("Other.lnk");
        std::fs::write(&other, "theirs").unwrap();
        uninstall(&menu).unwrap();
        assert_eq!(
            state(&menu, &executable).unwrap(),
            DesktopEntryState::NotInstalled
        );
        assert!(other.exists() && executable.exists());
        uninstall(&menu).unwrap();
    }
}
