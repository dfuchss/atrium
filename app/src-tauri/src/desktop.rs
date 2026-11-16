//! Tauri commands of the desktop integration (Settings, Linux): they show the state of the
//! desktop entry and install or remove it on request. Nothing is written at startup. Where
//! Atrium does not run as an `AppImage` (macOS, a source build) there is nothing to offer. They
//! also list the installed browsers for links.

use std::sync::Arc;

use protocol::{BrowserChoice, DesktopEntryState};
use tauri::State;
use updater::{RunningAppImage, running_appimage};

use crate::desktop_entry::{self, DesktopDirs};

/// The host log, for an icon announcement that failed.
pub(crate) struct DesktopLog(pub(crate) Arc<dyn Fn(&str) + Send + Sync>);

/// Lets the desktop see the changed icons; a failure only goes to the log.
fn announce_icons(log: &DesktopLog) {
    if let Err(error) = host_tauri::announce_new_icons() {
        (log.0)(&format!("cannot announce the changed icons: {error}"));
    }
}

fn state_of(dirs: &DesktopDirs, running: &RunningAppImage) -> Result<DesktopEntryState, String> {
    desktop_entry::state(dirs, &running.file)
}

#[tauri::command]
pub(crate) fn get_desktop_entry() -> Result<Option<DesktopEntryState>, String> {
    let Some(running) = running_appimage() else {
        return Ok(None);
    };
    state_of(&DesktopDirs::from_env()?, &running).map(Some)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]
#[tauri::command]
pub(crate) fn install_desktop_entry(
    log: State<'_, DesktopLog>,
) -> Result<DesktopEntryState, String> {
    let running = running_appimage().ok_or("Atrium does not run as an AppImage")?;
    let dirs = DesktopDirs::from_env()?;
    desktop_entry::install(&dirs, &running)?;
    announce_icons(&log);
    state_of(&dirs, &running)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]
#[tauri::command]
pub(crate) fn uninstall_desktop_entry(
    log: State<'_, DesktopLog>,
) -> Result<DesktopEntryState, String> {
    let running = running_appimage().ok_or("Atrium does not run as an AppImage")?;
    let dirs = DesktopDirs::from_env()?;
    desktop_entry::uninstall(&dirs)?;
    announce_icons(&log);
    state_of(&dirs, &running)
}

#[tauri::command]
pub(crate) fn list_browsers() -> Option<Vec<BrowserChoice>> {
    host_tauri::installed_browsers()
}
