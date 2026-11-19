//! Desktop integration: the desktop entry (launcher entry and icon) that Atrium installs for
//! itself on Linux, the Start menu shortcut on Windows, and the installed browsers to open links
//! in.
//!
//! - UI initial load: Tauri command [`CMD_GET_DESKTOP_ENTRY`] returns a
//!   `Result<Option<DesktopEntryState>, String>` (`None`: nothing to offer, because this is
//!   macOS or a build from source on Linux).
//! - UI -> backend: [`CMD_INSTALL_DESKTOP_ENTRY`] (also rewrites an outdated entry) and
//!   [`CMD_UNINSTALL_DESKTOP_ENTRY`] return the new `Result<DesktopEntryState, String>`.
//! - UI initial load: Tauri command [`CMD_LIST_BROWSERS`] returns an
//!   `Option<Vec<BrowserChoice>>` (`None`: the platform offers no list, the browser is typed by
//!   name).

use serde::{Deserialize, Serialize};

pub const CMD_GET_DESKTOP_ENTRY: &str = "get_desktop_entry";
pub const CMD_INSTALL_DESKTOP_ENTRY: &str = "install_desktop_entry";
pub const CMD_UNINSTALL_DESKTOP_ENTRY: &str = "uninstall_desktop_entry";
pub const CMD_LIST_BROWSERS: &str = "list_browsers";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DesktopEntryState {
    /// There is no entry.
    NotInstalled,
    /// The entry starts the running `AppImage` or release binary (Linux) or executable
    /// (Windows).
    Installed,
    /// The entry starts another file, e.g. before Atrium was moved or renamed.
    OtherPath { path: String },
}

/// An installed application that opens web links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserChoice {
    /// What the browser setting stores for it.
    pub id: String,
    /// Name shown in the settings.
    pub name: String,
}
