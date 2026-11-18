//! The app settings and the settings of each account.

use serde::{Deserialize, Serialize};

/// Width of the sidebar at the largest icon size (logical px). The actual width depends on
/// [`AppSettings::icon_size`], see [`IconSize::sidebar_width`].
const SIDEBAR_WIDTH: f64 = 72.0;

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
    /// The badge for `urgent` unread ([`AccountView::unread`](crate::AccountView::unread)) and
    /// `normal` unread ([`AccountView::indirect`](crate::AccountView::indirect)); urgent unread
    /// hides the normal dot.
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

/// Per-account settings (editable in the settings dialog), stored flat in each account of
/// `accounts.json`. Fields added after v0.1 have serde defaults so older files still load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountSettings {
    /// User-chosen label, e.g. "KIT" or "Work".
    pub label: String,
    /// Value for the service's `{instance}` placeholder (workspace, instance host...).
    #[serde(default)]
    pub instance: Option<String>,
    /// No notifications from this account (and not counted in the dock badge).
    #[serde(default)]
    pub muted: bool,
    /// Show the unread badge on the sidebar icon.
    #[serde(default = "yes")]
    pub show_badge: bool,
    /// Count this account's unread in the dock badge.
    #[serde(default = "yes")]
    pub in_dock_badge: bool,
    /// Overrides the service's user agent: "webkit" | "chrome" | "default" | literal; None = service default.
    #[serde(default)]
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
    /// Simple Icons slug (simpleicons.org, e.g. "discord"): a monochrome brand glyph drawn on the
    /// service color. Used when no `custom_icon` is set; None = site favicon.
    #[serde(default)]
    pub simple_icon: Option<String>,
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
