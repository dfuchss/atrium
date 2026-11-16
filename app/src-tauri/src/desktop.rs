//! Tauri commands of the desktop integration (Settings): they list the installed browsers for
//! links.

use protocol::BrowserChoice;

#[tauri::command]
pub(crate) fn list_browsers() -> Option<Vec<BrowserChoice>> {
    host_tauri::installed_browsers()
}
