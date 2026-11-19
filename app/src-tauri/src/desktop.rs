//! Tauri commands of the desktop integration (Settings): they show the state of the launcher
//! entry and install or remove it on request, the desktop entry of the `AppImage` or the release
//! binary on Linux (`desktop_entry`) and the Start menu shortcut on Windows (`start_menu`).
//! Nothing is written at startup. Where there is nothing to offer (macOS, a source build on
//! Linux) the state is `None`. They also list the installed browsers for links.

#![expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]

use protocol::{BrowserChoice, DesktopEntryState};
use tauri::{AppHandle, State};
use updater::LogSink;

/// The host log, for an icon announcement that failed (Linux) or a changed shortcut
/// (Windows).
pub(crate) struct DesktopLog(pub(crate) LogSink);

#[tauri::command]
pub(crate) fn get_desktop_entry(app: AppHandle) -> Result<Option<DesktopEntryState>, String> {
    platform::state(&app)
}

#[tauri::command]
pub(crate) fn install_desktop_entry(
    app: AppHandle,
    log: State<'_, DesktopLog>,
) -> Result<DesktopEntryState, String> {
    platform::install(&app, &log)
}

#[tauri::command]
pub(crate) fn uninstall_desktop_entry(
    app: AppHandle,
    log: State<'_, DesktopLog>,
) -> Result<DesktopEntryState, String> {
    platform::uninstall(&app, &log)
}

/// The desktop entry of the running `AppImage` or release binary.
#[cfg(not(windows))]
mod platform {
    use std::ffi::OsString;

    use fs_util::appimage::{AppImageEnv, Packaging};
    use protocol::DesktopEntryState;
    use tauri::AppHandle;

    use super::DesktopLog;
    use crate::desktop_entry::{self, DesktopDirs};

    const NOTHING_TO_OFFER: &str = "Atrium does not run as the AppImage or the downloaded program";

    /// Lets the desktop see the changed icons; a failure only goes to the log.
    fn announce_icons(log: &DesktopLog) {
        if let Err(error) = host_tauri::announce_new_icons() {
            (log.0)(&format!("cannot announce the changed icons: {error}"));
        }
    }

    /// The running `AppImage`, or on Linux the program outside a Cargo target folder; None
    /// where there is nothing to offer.
    fn running() -> Option<Packaging> {
        let packaging = AppImageEnv::from_env().ok()?.packaging();
        let offered = match &packaging {
            Packaging::AppImage(running) => running.file.is_absolute() && running.file.is_file(),
            Packaging::Binary(file) => cfg!(target_os = "linux") && !fs_util::in_cargo_target(file),
        };
        offered.then_some(packaging)
    }

    fn icons(packaging: &Packaging) -> Result<Vec<(OsString, Vec<u8>)>, String> {
        match packaging {
            Packaging::AppImage(running) => desktop_entry::appimage_icons(running),
            Packaging::Binary(_) => Ok(desktop_entry::embedded_icons()),
        }
    }

    pub(super) fn state(_app: &AppHandle) -> Result<Option<DesktopEntryState>, String> {
        let Some(running) = running() else {
            return Ok(None);
        };
        desktop_entry::state(&DesktopDirs::from_env()?, running.file()).map(Some)
    }

    pub(super) fn install(_app: &AppHandle, log: &DesktopLog) -> Result<DesktopEntryState, String> {
        let running = running().ok_or(NOTHING_TO_OFFER)?;
        let dirs = DesktopDirs::from_env()?;
        desktop_entry::install(&dirs, running.file(), &icons(&running)?)?;
        announce_icons(log);
        desktop_entry::state(&dirs, running.file())
    }

    pub(super) fn uninstall(
        _app: &AppHandle,
        log: &DesktopLog,
    ) -> Result<DesktopEntryState, String> {
        let running = running().ok_or(NOTHING_TO_OFFER)?;
        let dirs = DesktopDirs::from_env()?;
        desktop_entry::uninstall(&dirs)?;
        announce_icons(log);
        desktop_entry::state(&dirs, running.file())
    }
}

/// The Start menu shortcut of the running executable, named after the app's identifier.
#[cfg(windows)]
mod platform {
    use std::path::PathBuf;

    use protocol::DesktopEntryState;
    use tauri::{AppHandle, Manager as _};

    use super::DesktopLog;
    use crate::start_menu::{self, StartMenu};

    /// The user's Start menu and the running executable.
    fn locate(app: &AppHandle) -> Result<(StartMenu, PathBuf), String> {
        let roaming = app
            .path()
            .data_dir()
            .map_err(|error| format!("cannot find your app data folder: {error}"))?;
        let executable =
            std::env::current_exe().map_err(|error| format!("cannot locate Atrium: {error}"))?;
        Ok((StartMenu::in_roaming(&roaming), executable))
    }

    pub(super) fn state(app: &AppHandle) -> Result<Option<DesktopEntryState>, String> {
        let (menu, executable) = locate(app)?;
        start_menu::state(&menu, &executable).map(Some)
    }

    pub(super) fn install(app: &AppHandle, log: &DesktopLog) -> Result<DesktopEntryState, String> {
        let (menu, executable) = locate(app)?;
        start_menu::install(&menu, &executable, &app.config().identifier)?;
        (log.0)(&format!(
            "Start menu shortcut installed for {}",
            executable.display()
        ));
        start_menu::state(&menu, &executable)
    }

    pub(super) fn uninstall(
        app: &AppHandle,
        log: &DesktopLog,
    ) -> Result<DesktopEntryState, String> {
        let (menu, executable) = locate(app)?;
        start_menu::uninstall(&menu)?;
        (log.0)("Start menu shortcut removed");
        start_menu::state(&menu, &executable)
    }
}

#[tauri::command]
pub(crate) fn list_browsers() -> Option<Vec<BrowserChoice>> {
    host_tauri::installed_browsers()
}
