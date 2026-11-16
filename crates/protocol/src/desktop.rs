//! Desktop integration: the installed browsers to open links in.
//!
//! - UI initial load: Tauri command [`CMD_LIST_BROWSERS`] returns an
//!   `Option<Vec<BrowserChoice>>` (`None`: the platform offers no list, the browser is typed by
//!   name).

use serde::{Deserialize, Serialize};

pub const CMD_LIST_BROWSERS: &str = "list_browsers";

/// An installed application that opens web links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserChoice {
    /// What the browser setting stores for it.
    pub id: String,
    /// Name shown in the settings.
    pub name: String,
}
