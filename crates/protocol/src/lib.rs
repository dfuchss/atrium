//! Contract between the backend and the sidebar UI. Compiles to native and wasm32.
//!
//! - UI -> backend: Tauri command [`CMD_UI_COMMAND`] with argument `{ command: UiCommand }`.
//! - UI initial load: Tauri command [`CMD_GET_SNAPSHOT`] returns a [`Snapshot`].
//! - backend -> UI: event [`EVENT_SNAPSHOT`] (payload [`Snapshot`]) sent to the `ui` webview
//!   after every state change. The UI renders the snapshot; it keeps no state of its own
//!   besides form inputs.

mod desktop;
mod script_dialog;
mod update;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use desktop::*;
pub use script_dialog::*;
pub use update::*;

pub const CMD_UI_COMMAND: &str = "ui_command";
pub const CMD_GET_SNAPSHOT: &str = "get_snapshot";
/// Service definition editor commands (request/response, they bypass the core): arguments and
/// results are described on [`ServiceFile`] and [`ServiceProblem`].
pub const CMD_LIST_SERVICE_FILES: &str = "list_service_files";
pub const CMD_READ_SERVICE_FILE: &str = "read_service_file";
pub const CMD_NEW_SERVICE_TEMPLATE: &str = "new_service_template";
pub const CMD_VALIDATE_SERVICE: &str = "validate_service";
pub const CMD_SAVE_SERVICE_FILE: &str = "save_service_file";
pub const CMD_DELETE_SERVICE_FILE: &str = "delete_service_file";
pub const CMD_SET_SERVICE_FILE_ENABLED: &str = "set_service_file_enabled";
pub const EVENT_SNAPSHOT: &str = "snapshot";
/// Label of the local sidebar webview.
/// Project page (repository) of Atrium.
pub const PROJECT_URL: &str = "https://github.com/dfuchss/atrium";
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
    /// About tab: open [`PROJECT_URL`] in the browser.
    OpenProjectPage,
}

/// A file in the services folder: `<name>.toml` (enabled, loaded) or `<name>.toml.disabled`
/// (ignored). `name` is the file name on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceFile {
    pub name: String,
    pub enabled: bool,
}

/// A problem of a service definition text; `line` and `column` (1-based) are known when the
/// TOML parser reports a position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceProblem {
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
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
    /// The account whose settings the dialog opens with (account menu "Settings…"); set only
    /// in the snapshot that asks for it.
    pub edit_account: Option<Uuid>,
    pub settings: AppSettings,
    pub app: AppInfo,
    /// Problems of the last load of `<config>/services/*.toml` (startup or reload).
    pub service_errors: Vec<String>,
}

/// Appearance of the whole app (windows, menus, dialogs and the pages that honour
/// `prefers-color-scheme`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follow the system appearance: macOS, or the desktop's colour scheme on Linux.
    #[default]
    Auto,
    Light,
    Dark,
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
    /// Keep the sidebar unread badge of muted accounts (their unread never counts in the dock
    /// badge).
    pub badge_when_muted: bool,
    pub dock_badge: DockBadgeMode,
    pub launch_at_login: bool,
    /// Start with the window hidden (useful together with `launch_at_login`).
    pub start_hidden: bool,
    /// Closing the window hides it and keeps services running (macOS default); clicking the
    /// dock icon shows it again. Otherwise closing the window quits.
    pub close_to_background: bool,
    /// Application used for external links; None = system default. On macOS its name (e.g.
    /// "Firefox"), on Linux the desktop entry id of an installed browser (e.g. "firefox.desktop").
    pub browser: Option<String>,
    /// Where links that open a new window go (accounts can override it).
    pub link_target: LinkTarget,
    /// Menu bar (tray) icon with unread count and account menu.
    pub show_tray: bool,
    /// What happens to the loaded services when the computer wakes from sleep (accounts can
    /// override it: `AccountSettings::after_wake`).
    pub after_wake: AfterWake,
    /// Size of the sidebar icons (and thereby the sidebar width).
    pub icon_size: IconSize,
    /// How the sidebar icons show unread.
    pub badge_style: BadgeStyle,
    /// Put background accounts to sleep (unload their view) after this many minutes without
    /// being shown; None = never. Accounts can override it (`AccountSettings::hibernate_after_min`).
    /// Sleeping accounts get no notifications or unread updates until they are opened again.
    pub hibernate_after_min: Option<u32>,
    /// Auto, light or dark appearance.
    pub theme: Theme,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            dnd: false,
            notification_preview: true,
            notification_sound: true,
            badge_when_muted: true,
            dock_badge: DockBadgeMode::All,
            launch_at_login: false,
            start_hidden: false,
            close_to_background: true,
            browser: None,
            link_target: LinkTarget::SystemBrowser,
            show_tray: true,
            after_wake: AfterWake::Check,
            icon_size: IconSize::Small,
            badge_style: BadgeStyle::Count,
            hibernate_after_min: None,
            theme: Theme::Auto,
        }
    }
}

/// What happens to a loaded service when the computer wakes from sleep (on macOS once the
/// displays wake; a dark wake such as Power Nap does nothing).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AfterWake {
    /// Reload the service only if it does not answer a poll soon after the wake (most services
    /// reconnect by themselves).
    #[default]
    Check,
    /// Reload the service right away.
    Reload,
    /// Leave the service alone.
    Never,
}

impl AfterWake {
    pub const ALL: [AfterWake; 3] = [AfterWake::Check, AfterWake::Reload, AfterWake::Never];

    /// The stored name ("check", "reload", "never"), e.g. as form value.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            AfterWake::Check => "check",
            AfterWake::Reload => "reload",
            AfterWake::Never => "never",
        }
    }

    /// The mode stored as `name`, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<AfterWake> {
        AfterWake::ALL.into_iter().find(|mode| mode.name() == name)
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

/// Sidebar unread badges. Urgent unread (mentions, direct messages) is red; normal unread
/// (new messages in a subscribed channel) is a white dot and has no count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BadgeStyle {
    /// A red badge with the urgent count.
    #[default]
    Count,
    /// A red dot for urgent unread.
    Dot,
    /// A red badge with the urgent count; normal unread shows nothing.
    UrgentOnly,
}

impl BadgeStyle {
    /// The badge for `urgent` unread ([`AccountView::unread`]) and `normal` unread
    /// ([`AccountView::indirect`]); urgent unread hides the normal dot.
    pub fn badge(self, urgent: u32, normal: bool) -> Badge {
        match (urgent, self) {
            (0, BadgeStyle::Count | BadgeStyle::Dot) if normal => Badge::NormalDot,
            (0, _) => Badge::None,
            (count, BadgeStyle::Count | BadgeStyle::UrgentOnly) => Badge::Count(count),
            (_, BadgeStyle::Dot) => Badge::UrgentDot,
        }
    }
}

/// What a sidebar icon shows, see [`BadgeStyle::badge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    None,
    Count(u32),
    UrgentDot,
    NormalDot,
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
    /// Value for the service's `{instance}` placeholder (workspace, instance host...).
    pub instance: Option<String>,
    /// No notifications from this account.
    pub muted: bool,
    /// Show the unread badge on the sidebar icon.
    pub show_badge: bool,
    /// Count this account's unread in the dock badge.
    pub in_dock_badge: bool,
    /// Overrides the service's user agent: "webkit" | "chrome" | "default" | literal; None = service default.
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
    /// Don't load this account at startup; it loads when selected. Read at the next start only.
    #[serde(default)]
    pub start_asleep: bool,
    /// Overrides `AppSettings::after_wake`; None = use the global setting.
    #[serde(default)]
    pub after_wake: Option<AfterWake>,
    /// CSS injected into every page of this account (e.g. a dark theme or hiding clutter).
    #[serde(default)]
    pub custom_css: Option<String>,
    /// Grant camera/microphone/screen-capture requests of this account's pages.
    /// None = the service default (`allow_media` in the service definition; on for call apps).
    #[serde(default)]
    pub allow_media: Option<bool>,
    /// Hosts (exact or `*.domain`) whose new-window requests always open as in-app window,
    /// whatever the link target. Normalised by the core on update.
    #[serde(default)]
    pub app_hosts: Vec<String>,
}

impl AccountSettings {
    /// The sidebar icon shows the unread badge (`AppSettings::badge_when_muted` for muted accounts).
    #[must_use]
    pub fn shows_badge(&self, badge_when_muted: bool) -> bool {
        self.show_badge && (!self.muted || badge_when_muted)
    }
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
    /// May contain `{instance}`.
    pub url: String,
    /// CSS color for the initials icon.
    pub color: String,
    /// Ask for an instance value when adding an account (only meaningful if `url` has `{instance}`).
    pub instance_label: Option<String>,
    /// Regex on the page title; capture group 1 = unread count. None = `^\((\d+)\)`.
    pub title_regex: Option<String>,
    /// "webkit" | "chrome" | "default" | literal; None = "webkit".
    pub user_agent: Option<String>,
}

/// Operating system the host runs on; the UI words the settings after it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperatingSystem {
    #[default]
    MacOs,
    Linux,
    Windows,
}

/// Read-only information for the "About" section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    pub os: OperatingSystem,
    pub version: String,
    pub config_dir: String,
    /// Folder of the user's service definition files (`<config_dir>/services`).
    pub services_dir: String,
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
    /// Label of the service's instance field, if the service has one (e.g. "Instance").
    pub instance_label: Option<String>,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    /// When set, the add dialog asks for a `instance` value with this label (e.g. "Instance").
    pub instance_label: Option<String>,
    /// Pre-filled value for the instance field.
    pub instance_default: Option<String>,
    /// User-defined (editable/removable in settings).
    pub custom: bool,
    /// Service default for camera/microphone/screen-capture (see `AccountSettings::allow_media`).
    pub allow_media: bool,
    /// Definition details, so a custom service can start from this one as a template.
    pub url: String,
    pub title_regex: Option<String>,
    pub user_agent: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_shows_urgent_over_normal_unread() {
        assert_eq!(BadgeStyle::Count.badge(0, false), Badge::None);
        assert_eq!(BadgeStyle::Count.badge(0, true), Badge::NormalDot);
        assert_eq!(BadgeStyle::Count.badge(3, true), Badge::Count(3));
        assert_eq!(BadgeStyle::Dot.badge(0, false), Badge::None);
        assert_eq!(BadgeStyle::Dot.badge(0, true), Badge::NormalDot);
        assert_eq!(BadgeStyle::Dot.badge(3, true), Badge::UrgentDot);
        assert_eq!(BadgeStyle::UrgentOnly.badge(0, false), Badge::None);
        assert_eq!(BadgeStyle::UrgentOnly.badge(0, true), Badge::None);
        assert_eq!(BadgeStyle::UrgentOnly.badge(3, true), Badge::Count(3));
    }
}
