//! Persisted data.

use protocol::{AccountSettings, AppSettings, CustomService, LinkTarget};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One configured account = one service view with its own isolated data store.
/// Fields added after v0.1 have serde defaults so older `accounts.json` files still load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    /// Also the view id and (as bytes) the `WKWebView` data store identifier.
    pub id: Uuid,
    /// [`crate::ServiceDef::id`].
    pub service: String,
    pub label: String,
    #[serde(default)]
    pub instance: Option<String>,
    /// No notifications from this account (and not counted in the dock badge).
    #[serde(default)]
    pub muted: bool,
    /// Show the unread badge on the sidebar icon (UI only).
    #[serde(default = "yes")]
    pub show_badge: bool,
    /// Count this account's unread in the dock badge.
    #[serde(default = "yes")]
    pub in_dock_badge: bool,
    /// Overrides the service's user agent preset ("safari" | "chrome" | "default" | literal).
    #[serde(default)]
    pub user_agent: Option<String>,
    /// Disabled accounts keep their settings and login but get no view.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Overrides `AppSettings::link_target`.
    #[serde(default)]
    pub link_target: Option<LinkTarget>,
    /// User-chosen sidebar icon (`data:` or https URL).
    #[serde(default)]
    pub custom_icon: Option<String>,
    /// Page zoom factor, 1.0 = 100 %.
    #[serde(default = "one")]
    pub zoom: f64,
    /// Unload the view after this many minutes in the background; None = use the global
    /// setting, Some(0) = never.
    #[serde(default)]
    pub hibernate_after_min: Option<u32>,
    /// CSS injected into every page of this account.
    #[serde(default)]
    pub custom_css: Option<String>,
    /// Cached site favicon as `data:` URL (fetched once, refreshed on request).
    #[serde(default)]
    pub favicon: Option<String>,
    /// Grant camera/microphone/screen capture; None = the service default.
    #[serde(default)]
    pub allow_media: Option<bool>,
    /// Simple Icons slug (normalised) chosen by the user.
    #[serde(default)]
    pub simple_icon: Option<String>,
    /// Cached white glyph of `simple_icon` as `data:image/svg+xml` URL; cleared when the slug changes.
    #[serde(default)]
    pub simple_icon_data: Option<String>,
    /// Hosts whose new windows always open in the app (normalised).
    #[serde(default)]
    pub app_hosts: Vec<String>,
}

fn yes() -> bool {
    true
}

fn one() -> f64 {
    1.0
}

impl Account {
    /// New account with default settings.
    pub fn new(
        id: Uuid,
        service: impl Into<String>,
        label: impl Into<String>,
        instance: Option<String>,
    ) -> Account {
        Account {
            id,
            service: service.into(),
            label: label.into(),
            instance,
            muted: false,
            show_badge: true,
            in_dock_badge: true,
            user_agent: None,
            enabled: true,
            link_target: None,
            custom_icon: None,
            zoom: 1.0,
            hibernate_after_min: None,
            custom_css: None,
            favicon: None,
            allow_media: None,
            simple_icon: None,
            simple_icon_data: None,
            app_hosts: Vec::new(),
        }
    }

    /// 16-byte identifier of this account's isolated cookie/storage data store.
    pub fn store_id(&self) -> [u8; 16] {
        *self.id.as_bytes()
    }

    /// The user-editable part, as shown in the settings dialog.
    pub fn settings(&self) -> AccountSettings {
        AccountSettings {
            label: self.label.clone(),
            instance: self.instance.clone(),
            muted: self.muted,
            show_badge: self.show_badge,
            in_dock_badge: self.in_dock_badge,
            user_agent: self.user_agent.clone(),
            enabled: self.enabled,
            link_target: self.link_target,
            custom_icon: self.custom_icon.clone(),
            zoom: self.zoom,
            hibernate_after_min: self.hibernate_after_min,
            custom_css: self.custom_css.clone(),
            allow_media: self.allow_media,
            simple_icon: self.simple_icon.clone(),
            app_hosts: self.app_hosts.clone(),
        }
    }

    /// Stores already validated settings; a new Simple Icons slug drops the cached glyph.
    pub fn apply_settings(&mut self, settings: AccountSettings) {
        if self.simple_icon != settings.simple_icon {
            self.simple_icon_data = None;
        }
        self.label = settings.label;
        self.instance = settings.instance;
        self.muted = settings.muted;
        self.show_badge = settings.show_badge;
        self.in_dock_badge = settings.in_dock_badge;
        self.user_agent = settings.user_agent;
        self.enabled = settings.enabled;
        self.link_target = settings.link_target;
        self.custom_icon = settings.custom_icon;
        self.zoom = settings.zoom;
        self.hibernate_after_min = settings.hibernate_after_min;
        self.custom_css = settings.custom_css;
        self.allow_media = settings.allow_media;
        self.simple_icon = settings.simple_icon;
        self.app_hosts = settings.app_hosts;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Global app settings, stored flat next to `active` (older files only have `dnd`).
    #[serde(flatten)]
    pub app: AppSettings,
    /// Account shown at startup (last selected).
    #[serde(default)]
    pub active: Option<Uuid>,
}

/// Everything that is saved to disk. Sidebar order = order of `accounts`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Persisted {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub settings: Settings,
    /// User-defined services (added in the settings dialog).
    #[serde(default)]
    pub custom_services: Vec<CustomService>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::DockBadgeMode;

    #[test]
    fn loads_old_format() {
        let json = r#"{
            "accounts": [
                {"id": "00000000-0000-0000-0000-00000000002a", "service": "slack", "label": "KIT", "instance": "kit", "muted": true},
                {"id": "00000000-0000-0000-0000-00000000002b", "service": "discord", "label": "Discord"}
            ],
            "settings": {"dnd": true, "active": "00000000-0000-0000-0000-00000000002a"}
        }"#;
        let p: Persisted = serde_json::from_str(json).unwrap();
        let a = &p.accounts[0];
        assert_eq!(
            (a.label.as_str(), a.instance.as_deref(), a.muted),
            ("KIT", Some("kit"), true)
        );
        assert!(a.show_badge && a.in_dock_badge && a.user_agent.is_none());
        assert!(
            a.enabled
                && a.link_target.is_none()
                && a.custom_icon.is_none()
                && a.custom_css.is_none()
        );
        assert_eq!(
            (a.zoom, a.hibernate_after_min, a.favicon.as_deref()),
            (1.0, None, None)
        );
        assert!(p.custom_services.is_empty());
        assert_eq!(a.allow_media, None);
        assert!(a.app_hosts.is_empty());
        assert_eq!(
            (a.simple_icon.as_deref(), a.simple_icon_data.as_deref()),
            (None, None)
        );
        assert!(!p.accounts[1].muted && p.accounts[1].instance.is_none());
        assert_eq!(p.settings.active, Some(Uuid::from_u128(42)));
        let expected = AppSettings {
            dnd: true,
            ..AppSettings::default()
        };
        assert_eq!(p.settings.app, expected);
    }

    #[test]
    fn empty_and_round_trip() {
        let p: Persisted = serde_json::from_str("{}").unwrap();
        assert_eq!(p, Persisted::default());
        let mut p = Persisted {
            accounts: vec![Account::new(Uuid::from_u128(1), "slack", "S", None)],
            ..Default::default()
        };
        p.accounts[0].user_agent = Some("chrome".into());
        p.settings.app.dock_badge = DockBadgeMode::CountOnly;
        p.settings.app.browser = Some("Firefox".into());
        p.settings.active = Some(Uuid::from_u128(1));
        p.settings.app.link_target = LinkTarget::AppWindow;
        let a = &mut p.accounts[0];
        a.enabled = false;
        a.link_target = Some(LinkTarget::SystemBrowser);
        a.zoom = 1.25;
        a.hibernate_after_min = Some(10);
        a.custom_css = Some("body{color:red}".into());
        a.favicon = Some("data:image/png;base64,AA==".into());
        a.app_hosts = vec!["*.example.org".into()];
        a.simple_icon = Some("discord".into());
        a.simple_icon_data = Some("data:image/svg+xml;base64,AA==".into());
        p.custom_services.push(CustomService {
            id: "custom-x".into(),
            name: "X".into(),
            url: "https://x.io/".into(),
            color: "#000".into(),
            instance_label: None,
            title_regex: None,
            user_agent: None,
        });
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""dock_badge":"count_only""#), "{json}");
        assert!(json.contains(r#""link_target":"app_window""#), "{json}");
        assert_eq!(serde_json::from_str::<Persisted>(&json).unwrap(), p);
    }

    #[test]
    fn apply_settings_round_trips_and_drops_stale_glyph() {
        let mut account = Account::new(Uuid::from_u128(1), "slack", "Work", None);
        account.simple_icon = Some("slack".into());
        account.simple_icon_data = Some("data:image/svg+xml;base64,AA".into());
        let mut settings = account.settings();
        settings.label = "Home".into();
        account.apply_settings(settings.clone());
        assert_eq!(account.settings(), settings);
        assert!(
            account.simple_icon_data.is_some(),
            "same slug keeps the glyph"
        );

        settings.simple_icon = Some("discord".into());
        account.apply_settings(settings);
        assert_eq!(account.simple_icon.as_deref(), Some("discord"));
        assert_eq!(account.simple_icon_data, None);
    }
}
