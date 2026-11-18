//! Persisted data.

use protocol::{AccountSettings, AfterWake, AppSettings, CustomService};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One configured account = one service view with its own isolated data store. Stored with
/// its settings flat beside the fields the user does not edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    /// Also the view id and (as bytes) the `WKWebView` data store identifier.
    pub id: Uuid,
    /// [`crate::ServiceDef::id`].
    pub service: String,
    #[serde(flatten)]
    pub settings: AccountSettings,
    /// Cached site favicon as `data:` URL (fetched once, refreshed on request).
    #[serde(default)]
    pub favicon: Option<String>,
    /// Cached white glyph of `simple_icon` as `data:image/svg+xml` URL; cleared when the slug changes.
    #[serde(default)]
    pub simple_icon_data: Option<String>,
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
            settings: AccountSettings {
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
                start_asleep: false,
                after_wake: None,
                custom_css: None,
                allow_media: None,
                simple_icon: None,
                app_hosts: Vec::new(),
            },
            favicon: None,
            simple_icon_data: None,
        }
    }

    /// 16-byte identifier of this account's isolated cookie/storage data store.
    pub fn store_id(&self) -> [u8; 16] {
        *self.id.as_bytes()
    }

    /// Stores already validated settings; a new Simple Icons slug drops the cached glyph.
    pub fn apply_settings(&mut self, settings: AccountSettings) {
        if self.settings.simple_icon != settings.simple_icon {
            self.simple_icon_data = None;
        }
        self.settings = settings;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "StoredSettings")]
pub struct Settings {
    /// Global app settings, stored flat next to `active` (older files only have `dnd`).
    #[serde(flatten)]
    pub app: AppSettings,
    /// Account shown at startup (last selected).
    #[serde(default)]
    pub active: Option<Uuid>,
}

/// [`Settings`] as read from disk, with the fields of older versions that are migrated on load.
#[derive(Deserialize)]
struct StoredSettings {
    #[serde(flatten)]
    app: AppSettings,
    #[serde(default)]
    active: Option<Uuid>,
    /// Replaced by `after_wake`: true reloaded every service after a wake, false none.
    #[serde(default)]
    reload_after_wake: Option<bool>,
}

impl From<StoredSettings> for Settings {
    fn from(stored: StoredSettings) -> Settings {
        let mut app = stored.app;
        if let Some(reload) = stored.reload_after_wake {
            // Services that answer after a wake need no reload, so a reload becomes a check.
            app.after_wake = if reload {
                AfterWake::Check
            } else {
                AfterWake::Never
            };
        }
        Settings {
            app,
            active: stored.active,
        }
    }
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

/// The data stores among `existing` that belong to none of `accounts` (left by removed
/// accounts whose deletion failed).
pub fn unused_store_ids(existing: &[[u8; 16]], accounts: &[Account]) -> Vec<[u8; 16]> {
    existing
        .iter()
        .filter(|store_id| {
            !accounts
                .iter()
                .any(|account| account.store_id() == **store_id)
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{DockBadgeMode, LinkTarget};

    #[test]
    fn unused_stores_are_those_without_account() {
        let kept = Account::new(Uuid::from_u128(1), "slack", "S", None);
        let existing = [kept.store_id(), *Uuid::from_u128(2).as_bytes()];
        assert_eq!(
            unused_store_ids(&existing, &[kept]),
            vec![*Uuid::from_u128(2).as_bytes()]
        );
        assert!(unused_store_ids(&[], &[]).is_empty());
    }

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
            (
                a.settings.label.as_str(),
                a.settings.instance.as_deref(),
                a.settings.muted
            ),
            ("KIT", Some("kit"), true)
        );
        assert!(
            a.settings.show_badge && a.settings.in_dock_badge && a.settings.user_agent.is_none()
        );
        assert!(
            a.settings.enabled
                && a.settings.link_target.is_none()
                && a.settings.custom_icon.is_none()
                && a.settings.custom_css.is_none()
        );
        assert_eq!(
            (
                a.settings.zoom,
                a.settings.hibernate_after_min,
                a.favicon.as_deref()
            ),
            (1.0, None, None)
        );
        assert!(p.custom_services.is_empty());
        assert_eq!(a.settings.allow_media, None);
        assert!(a.settings.app_hosts.is_empty() && !a.settings.start_asleep);
        assert_eq!(
            (
                a.settings.simple_icon.as_deref(),
                a.simple_icon_data.as_deref()
            ),
            (None, None)
        );
        assert!(!p.accounts[1].settings.muted && p.accounts[1].settings.instance.is_none());
        assert_eq!(p.settings.active, Some(Uuid::from_u128(42)));
        let expected = AppSettings {
            dnd: true,
            ..AppSettings::default()
        };
        assert_eq!(p.settings.app, expected);
    }

    /// The keys and values an account had on disk before its settings were a struct of their
    /// own, so files stay readable by older versions.
    #[test]
    fn account_keeps_its_stored_format() {
        let mut account = Account::new(Uuid::from_u128(42), "slack", "KIT", Some("kit".into()));
        account.settings.link_target = Some(LinkTarget::AppWindow);
        account.settings.hibernate_after_min = Some(10);
        account.settings.after_wake = Some(AfterWake::Reload);
        account.settings.allow_media = Some(true);
        account.settings.simple_icon = Some("slack".into());
        account.settings.app_hosts = vec!["*.example.org".into()];
        account.favicon = Some("data:image/png;base64,AA==".into());
        let old_format = serde_json::json!({
            "id": "00000000-0000-0000-0000-00000000002a",
            "service": "slack",
            "label": "KIT",
            "instance": "kit",
            "muted": false,
            "show_badge": true,
            "in_dock_badge": true,
            "user_agent": null,
            "enabled": true,
            "link_target": "app_window",
            "custom_icon": null,
            "zoom": 1.0,
            "hibernate_after_min": 10,
            "start_asleep": false,
            "after_wake": "reload",
            "custom_css": null,
            "favicon": "data:image/png;base64,AA==",
            "allow_media": true,
            "simple_icon": "slack",
            "simple_icon_data": null,
            "app_hosts": ["*.example.org"]
        });
        assert_eq!(serde_json::to_value(&account).unwrap(), old_format);
        assert_eq!(
            serde_json::from_value::<Account>(old_format).unwrap(),
            account
        );
    }

    #[test]
    fn empty_and_round_trip() {
        let p: Persisted = serde_json::from_str("{}").unwrap();
        assert_eq!(p, Persisted::default());
        let mut p = Persisted {
            accounts: vec![Account::new(Uuid::from_u128(1), "slack", "S", None)],
            ..Default::default()
        };
        p.accounts[0].settings.user_agent = Some("chrome".into());
        p.settings.app.dock_badge = DockBadgeMode::CountOnly;
        p.settings.app.browser = Some("Firefox".into());
        p.settings.active = Some(Uuid::from_u128(1));
        p.settings.app.link_target = LinkTarget::AppWindow;
        let a = &mut p.accounts[0];
        a.settings.enabled = false;
        a.settings.link_target = Some(LinkTarget::SystemBrowser);
        a.settings.zoom = 1.25;
        a.settings.hibernate_after_min = Some(10);
        a.settings.start_asleep = true;
        a.settings.custom_css = Some("body{color:red}".into());
        a.favicon = Some("data:image/png;base64,AA==".into());
        a.settings.app_hosts = vec!["*.example.org".into()];
        a.settings.simple_icon = Some("discord".into());
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
    fn muted_account_shows_badge_only_if_kept_when_muted() {
        let mut account = Account::new(Uuid::from_u128(1), "slack", "S", None);
        assert!(account.settings.shows_badge(false));
        account.settings.muted = true;
        assert!(account.settings.shows_badge(true));
        assert!(!account.settings.shows_badge(false));
        account.settings.show_badge = false;
        assert!(!account.settings.shows_badge(true));
    }

    #[test]
    fn apply_settings_round_trips_and_drops_stale_glyph() {
        let mut account = Account::new(Uuid::from_u128(1), "slack", "Work", None);
        account.settings.simple_icon = Some("slack".into());
        account.simple_icon_data = Some("data:image/svg+xml;base64,AA".into());
        let mut settings = account.settings.clone();
        settings.label = "Home".into();
        account.apply_settings(settings.clone());
        assert_eq!(account.settings, settings);
        assert!(
            account.simple_icon_data.is_some(),
            "same slug keeps the glyph"
        );

        settings.simple_icon = Some("discord".into());
        account.apply_settings(settings);
        assert_eq!(account.settings.simple_icon.as_deref(), Some("discord"));
        assert_eq!(account.simple_icon_data, None);
    }
}
