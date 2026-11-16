//! The updater's contract with the UI. It is kept apart from [`crate::Snapshot`] because the
//! updater runs beside the core, not inside it.
//!
//! - UI -> backend: Tauri command [`CMD_UPDATE_COMMAND`] with argument `{ command: UpdateCommand }`.
//! - UI initial load: Tauri command [`CMD_GET_UPDATE_STATUS`] returns an `Option<UpdateStatus>`
//!   (`None`: this build has no updater).
//! - backend -> UI: event [`EVENT_UPDATE_STATUS`] (payload [`UpdateStatus`]) after every change.

use serde::{Deserialize, Serialize};

pub const CMD_UPDATE_COMMAND: &str = "update_command";
pub const CMD_GET_UPDATE_STATUS: &str = "get_update_status";
pub const EVENT_UPDATE_STATUS: &str = "update_status";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpdateCommand {
    /// Ask GitHub for the latest release now.
    CheckNow,
    /// Turn the automatic check on startup and every few hours on or off.
    SetAutoCheck { enabled: bool },
    /// Quit, swap in the staged update and start it (only while [`UpdateState::Ready`]).
    RestartToUpdate,
    /// Open the release page of the offered update (or the latest release) in the browser.
    OpenReleasePage,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpdateState {
    /// No check ran yet in this session.
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer release exists, but this installation cannot update itself (see
    /// [`UpdateStatus::manual_reason`]).
    Available {
        version: String,
    },
    /// `percent` is `None` while the size is unknown.
    Downloading {
        version: String,
        percent: Option<u8>,
    },
    /// Verified and staged next to the installation; a restart installs it.
    Ready {
        version: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub state: UpdateState,
    /// Check on startup and periodically.
    pub auto_check: bool,
    /// Why updates have to be installed by hand (development build, not in Applications...),
    /// as a whole sentence; `None` when Atrium can update itself.
    pub manual_reason: Option<String>,
    /// The previous "Restart and update" did not install the update, with the reason.
    pub last_failure: Option<String>,
}

impl UpdateStatus {
    /// Whether an update is waiting for the user (shown as a dot on the settings button).
    pub fn has_update(&self) -> bool {
        matches!(
            self.state,
            UpdateState::Available { .. } | UpdateState::Ready { .. }
        )
    }
}
