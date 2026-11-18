//! What the UI asks the backend to do.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AccountSettings, AppSettings, CustomService};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiCommand {
    /// Show this account's view.
    Select {
        account: Uuid,
    },
    /// `instance` fills the `{instance}` placeholder of the service URL (workspace, instance host...).
    AddAccount {
        service: String,
        label: String,
        instance: Option<String>,
    },
    RemoveAccount {
        account: Uuid,
    },
    SetMuted {
        account: Uuid,
        muted: bool,
    },
    /// Move the account up (negative) or down (positive) in the sidebar.
    Move {
        account: Uuid,
        delta: i32,
    },
    Reload {
        account: Uuid,
    },
    /// Reload bypassing the cache (every resource revalidated with the server).
    ForceReload {
        account: Uuid,
    },
    /// The manage/add dialog covers the whole window: service views are hidden while it is open.
    SetDialog {
        open: bool,
    },
    /// Do not disturb: suppresses all native notifications (quick toggle of `AppSettings::dnd`).
    SetDnd {
        enabled: bool,
    },
    /// Replace the app settings (settings dialog "General", "Notifications", "Links").
    UpdateSettings {
        settings: AppSettings,
    },
    /// Replace one account's settings. A changed `instance` or `user_agent` recreates the view
    /// (same data store, so the login normally survives).
    UpdateAccount {
        account: Uuid,
        settings: AccountSettings,
    },
    /// History navigation in the account's view.
    GoBack {
        account: Uuid,
    },
    GoForward {
        account: Uuid,
    },
    /// Open the page currently shown in the account's view in the browser.
    OpenInBrowser {
        account: Uuid,
    },
    /// Delete cookies/storage/cache of the account (logs it out) and reload.
    ClearData {
        account: Uuid,
    },
    /// Unload the view now to save memory; selecting the account loads it again.
    Hibernate {
        account: Uuid,
    },
    /// Right-click on the account's sidebar icon: show its native menu (sleep or wake up,
    /// settings, notifications on or off, reload).
    ShowAccountMenu {
        account: Uuid,
    },
    /// Fetch the site's favicon again.
    RefreshIcon {
        account: Uuid,
    },
    /// User-defined service ("custom app / site"); `id` is assigned by the backend when empty.
    AddCustomService {
        service: CustomService,
    },
    UpdateCustomService {
        service: CustomService,
    },
    /// Fails (logged, no change) while accounts still use the service.
    RemoveCustomService {
        id: String,
    },
    /// Expert mode: open `<config>/services/` in the file manager (created with a README and an
    /// example on first use).
    OpenServicesFolder,
    /// Expert mode: re-read the TOML files in `<config>/services/` without a restart.
    ReloadServices,
    /// About tab: open [`PROJECT_URL`](crate::PROJECT_URL) in the browser.
    OpenProjectPage,
}
