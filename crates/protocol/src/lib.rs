//! Contract between the backend and the sidebar UI. Compiles to native and wasm32.
//!
//! - UI -> backend: Tauri command [`CMD_UI_COMMAND`] with argument `{ cmd: UiCommand }`.
//! - UI initial load: Tauri command [`CMD_GET_SNAPSHOT`] returns a [`Snapshot`].
//! - backend -> UI: event [`EVENT_SNAPSHOT`] (payload [`Snapshot`]) sent to the `ui` webview
//!   after every state change. The UI renders the snapshot; it keeps no state of its own
//!   besides form inputs.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const CMD_UI_COMMAND: &str = "ui_command";
pub const CMD_GET_SNAPSHOT: &str = "get_snapshot";
pub const EVENT_SNAPSHOT: &str = "snapshot";
/// Label of the local sidebar webview.
pub const UI_WEBVIEW: &str = "ui";
/// Width of the sidebar at the largest icon size (logical px). The actual width depends on
/// [`AppSettings::icon_size`], see [`IconSize::sidebar_width`].
pub const SIDEBAR_WIDTH: f64 = 72.0;

/// Size of the account icons in the sidebar; the sidebar width follows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconSize {
    #[default]
    Small,
    Medium,
    Large,
}

impl IconSize {
    /// Edge length of an account icon in logical px.
    pub fn icon_px(self) -> f64 {
        match self {
            IconSize::Small => 32.0,
            IconSize::Medium => 38.0,
            IconSize::Large => 44.0,
        }
    }

    /// Width of the sidebar webview in logical px; service views start right of it.
    pub fn sidebar_width(self) -> f64 {
        match self {
            IconSize::Small => 56.0,
            IconSize::Medium => 64.0,
            IconSize::Large => SIDEBAR_WIDTH,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiCommand {
    /// Show this account's view.
    Select {
        account: Uuid,
    },
    /// `team` fills the `{team}` placeholder of the service URL (workspace, instance host...).
    AddAccount {
        service: String,
        label: String,
        team: Option<String>,
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
    /// Replace one account's settings. A changed `team` or `user_agent` recreates the view
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
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// In sidebar order.
    pub accounts: Vec<AccountView>,
    pub active: Option<Uuid>,
    /// Services that can be added.
    pub services: Vec<ServiceInfo>,
    /// The user-defined services (also contained in `services`, with `custom: true`).
    pub custom_services: Vec<CustomService>,
    pub dialog_open: bool,
    pub settings: AppSettings,
    pub app: AppInfo,
}

/// Global settings, persisted by the backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Do not disturb: no native notifications at all.
    pub dnd: bool,
    /// Show the message text in notifications; otherwise only "New message".
    pub notification_preview: bool,
    pub notification_sound: bool,
    pub dock_badge: DockBadgeMode,
    pub launch_at_login: bool,
    /// Start with the window hidden (useful together with `launch_at_login`).
    pub start_hidden: bool,
    /// Closing the window hides it and keeps services running (macOS default); clicking the
    /// dock icon shows it again. Otherwise closing the window quits.
    pub close_to_background: bool,
    /// Application used for external links, e.g. "Firefox" or "Safari"; None = system default.
    pub browser: Option<String>,
    /// Where links that open a new window go (accounts can override it).
    pub link_target: LinkTarget,
    /// Menu bar (tray) icon with unread count and account menu.
    pub show_tray: bool,
    /// Reload all loaded services after the Mac wakes from sleep (stale connections).
    pub reload_after_wake: bool,
    /// Size of the sidebar icons (and thereby the sidebar width).
    pub icon_size: IconSize,
    /// Put background accounts to sleep (unload their view) after this many minutes without
    /// being shown; None = never. Accounts can override it (`AccountSettings::hibernate_after_min`).
    /// Sleeping accounts get no notifications or unread updates until they are opened again.
    pub hibernate_after_min: Option<u32>,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            dnd: false,
            notification_preview: true,
            notification_sound: true,
            dock_badge: DockBadgeMode::All,
            launch_at_login: false,
            start_hidden: false,
            close_to_background: true,
            browser: None,
            link_target: LinkTarget::SystemBrowser,
            show_tray: true,
            reload_after_wake: true,
            icon_size: IconSize::Small,
            hibernate_after_min: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockBadgeMode {
    /// Count of direct unread, or a dot when only indirect unread exists.
    #[default]
    All,
    /// Only the count; indirect unread shows nothing.
    CountOnly,
    Off,
}

/// Where a link that wants a new window is opened. Sign-in popups (window.open with an explicit
/// size) always stay in the app so OAuth/SSO flows keep working.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkTarget {
    #[default]
    SystemBrowser,
    /// A separate app window that shares the account's session.
    AppWindow,
}

/// Per-account settings (editable in the settings dialog).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountSettings {
    /// User-chosen label, e.g. "KIT" or "Work".
    pub label: String,
    /// Value for the service's `{team}` placeholder (workspace, instance host...).
    pub team: Option<String>,
    /// No notifications from this account.
    pub muted: bool,
    /// Show the unread badge on the sidebar icon.
    pub show_badge: bool,
    /// Count this account's unread in the dock badge.
    pub in_dock_badge: bool,
    /// Overrides the service's user agent: "safari" | "chrome" | "default" | literal; None = service default.
    pub user_agent: Option<String>,
    /// Disabled accounts keep their settings and login but have no view (not loaded at all).
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Overrides `AppSettings::link_target`; None = use the global setting.
    #[serde(default)]
    pub link_target: Option<LinkTarget>,
    /// Custom sidebar icon: `data:` URL (uploaded image) or https URL; None = site favicon.
    #[serde(default)]
    pub custom_icon: Option<String>,
    /// Simple Icons slug (simpleicons.org, e.g. "discord"): a monochrome brand glyph drawn on the
    /// service color. Used when no `custom_icon` is set; None = site favicon.
    #[serde(default)]
    pub simple_icon: Option<String>,
    /// Page zoom factor, 1.0 = 100 %.
    #[serde(default = "one")]
    pub zoom: f64,
    /// Unload the view after this many minutes in the background.
    /// None = use `AppSettings::hibernate_after_min`; Some(0) = never for this account.
    #[serde(default)]
    pub hibernate_after_min: Option<u32>,
    /// CSS injected into every page of this account (e.g. a dark theme or hiding clutter).
    #[serde(default)]
    pub custom_css: Option<String>,
    /// Grant camera/microphone/screen-capture requests of this account's pages.
    /// None = the service default (`allow_media` in the service definition; on for call apps).
    #[serde(default)]
    pub allow_media: Option<bool>,
}

fn yes() -> bool {
    true
}

fn one() -> f64 {
    1.0
}

/// A user-defined service. Same meaning as the fields of the TOML service definitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomService {
    /// Empty when adding; the backend assigns `custom-<slug>`.
    pub id: String,
    pub name: String,
    /// May contain `{team}`.
    pub url: String,
    /// CSS color for the initials icon.
    pub color: String,
    /// Ask for a team value when adding an account (only meaningful if `url` has `{team}`).
    pub team_label: Option<String>,
    /// Regex on the page title; capture group 1 = unread count. None = `^\((\d+)\)`.
    pub title_regex: Option<String>,
    /// "safari" | "chrome" | "default" | literal; None = "safari".
    pub user_agent: Option<String>,
}

/// Read-only information for the "About" section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    pub version: String,
    pub config_dir: String,
    pub log_file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountView {
    pub id: Uuid,
    pub service: String,
    pub service_name: String,
    /// CSS color of the service, e.g. "#4A154B".
    pub color: String,
    /// One or two letters shown in the sidebar icon.
    pub initials: String,
    /// Direct unread count (mentions, DMs).
    pub unread: u32,
    /// Something unread without a count (channel activity) -> show a dot.
    pub indirect: bool,
    /// Label of the service's team field, if the service has one (e.g. "Instance").
    pub team_label: Option<String>,
    /// Image for the sidebar icon (custom icon, else cached favicon) as `data:`/https URL;
    /// None = draw `initials` on `color`.
    pub icon: Option<String>,
    /// `icon` is a monochrome glyph (Simple Icons, white): draw it padded on `color` instead of
    /// filling the icon box.
    pub icon_glyph: bool,
    /// The view is currently unloaded (hibernated or disabled).
    pub hibernated: bool,
    pub settings: AccountSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    /// When set, the add dialog asks for a `team` with this label (e.g. "Instance").
    pub team_label: Option<String>,
    /// Pre-filled value for the team field.
    pub team_default: Option<String>,
    /// User-defined (editable/removable in settings).
    pub custom: bool,
    /// Service default for camera/microphone/screen-capture (see `AccountSettings::allow_media`).
    pub allow_media: bool,
    /// Definition details, so a custom service can start from this one as a template.
    pub url: String,
    pub title_regex: Option<String>,
    pub user_agent: Option<String>,
}
