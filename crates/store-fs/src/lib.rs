//! File-based [`domain::ports::Store`]: `<dir>/accounts.json`.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use domain::Persisted;
use domain::ports::{PortError, PortResult, Store};

pub struct FsStore {
    dir: PathBuf,
}

impl FsStore {
    pub fn new(dir: impl Into<PathBuf>) -> FsStore {
        FsStore { dir: dir.into() }
    }

    fn file(&self) -> PathBuf {
        self.dir.join("accounts.json")
    }

    /// True if `accounts.json` exists and no earlier version was set aside as corrupt: only
    /// then does it name every account whose web data may still be in use.
    pub fn lists_all_accounts(&self) -> bool {
        let quarantined = fs::read_dir(&self.dir).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(CORRUPT_FILE_PREFIX)
            })
        });
        self.file().is_file() && !quarantined
    }
}

/// Name prefix of a corrupt `accounts.json` kept for inspection.
const CORRUPT_FILE_PREFIX: &str = "accounts.json.corrupt-";

fn port_error(action: &str, error: impl std::fmt::Display) -> PortError {
    PortError(format!("{action}: {error}"))
}

impl Store for FsStore {
    fn load(&self) -> PortResult<Persisted> {
        let path = self.file();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Persisted::default());
            }
            Err(error) => return Err(port_error("read accounts.json", error)),
        };
        if let Ok(persisted) = serde_json::from_slice(&bytes) {
            Ok(persisted)
        } else {
            // Keep the unreadable file for inspection and start empty.
            let seconds = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since_epoch| since_epoch.as_secs());
            let target = self.dir.join(format!("{CORRUPT_FILE_PREFIX}{seconds}"));
            fs::rename(&path, &target)
                .map_err(|error| port_error("quarantine corrupt accounts.json", error))?;
            Ok(Persisted::default())
        }
    }

    fn save(&self, data: &Persisted) -> PortResult {
        fs::create_dir_all(&self.dir).map_err(|error| port_error("create dir", error))?;
        let json =
            serde_json::to_vec_pretty(data).map_err(|error| port_error("serialize", error))?;
        let temporary = self.dir.join("accounts.json.tmp");
        let mut file =
            fs::File::create(&temporary).map_err(|error| port_error("create tmp", error))?;
        file.write_all(&json)
            .map_err(|error| port_error("write tmp", error))?;
        file.sync_all()
            .map_err(|error| port_error("sync tmp", error))?;
        drop(file);
        fs::rename(&temporary, self.file()).map_err(|error| port_error("rename", error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Account, Settings};
    use protocol::AppSettings;

    fn sample() -> Persisted {
        let id = uuid_like();
        Persisted {
            accounts: vec![Account {
                muted: true,
                in_dock_badge: false,
                user_agent: Some("chrome".into()),
                ..Account::new(id, "slack", "KIT", Some("x".into()))
            }],
            settings: Settings {
                app: AppSettings {
                    dnd: true,
                    browser: Some("Firefox".into()),
                    ..AppSettings::default()
                },
                active: Some(id),
            },
            custom_services: vec![],
        }
    }

    #[test]
    fn loads_old_format_file() {
        let d = tempfile::tempdir().unwrap();
        let old = r#"{"accounts":[{"id":"00000000-0000-0000-0000-00000000002a","service":"slack","label":"KIT","instance":"x","muted":true}],"settings":{"dnd":true,"active":"00000000-0000-0000-0000-00000000002a"}}"#;
        fs::write(d.path().join("accounts.json"), old).unwrap();
        let p = FsStore::new(d.path()).load().unwrap();
        let id = uuid_like();
        let expected = Persisted {
            accounts: vec![Account {
                muted: true,
                ..Account::new(id, "slack", "KIT", Some("x".into()))
            }],
            settings: Settings {
                app: AppSettings {
                    dnd: true,
                    ..AppSettings::default()
                },
                active: Some(id),
            },
            custom_services: vec![],
        };
        assert_eq!(p, expected);
        assert!(
            d.path().join("accounts.json").exists(),
            "old file must not be quarantined"
        );
    }

    /// File written by the previous version (per-account settings, no batch-2 fields).
    #[test]
    fn loads_previous_version_file() {
        let d = tempfile::tempdir().unwrap();
        let prev = r#"{
          "accounts": [
            {"id": "00000000-0000-0000-0000-00000000002a", "service": "rainbow", "label": "Work", "instance": null,
             "muted": false, "show_badge": false, "in_dock_badge": true, "user_agent": "chrome"}
          ],
          "settings": {"dnd": false, "notification_preview": false, "notification_sound": true,
                       "dock_badge": "count_only", "launch_at_login": true, "start_hidden": false,
                       "close_to_background": true, "browser": "Firefox",
                       "active": "00000000-0000-0000-0000-00000000002a"}
        }"#;
        fs::write(d.path().join("accounts.json"), prev).unwrap();
        let p = FsStore::new(d.path()).load().unwrap();
        let a = &p.accounts[0];
        assert_eq!(
            (a.label.as_str(), a.show_badge, a.user_agent.as_deref()),
            ("Work", false, Some("chrome"))
        );
        assert!(
            a.enabled && a.link_target.is_none() && a.custom_icon.is_none() && a.favicon.is_none()
        );
        assert_eq!(
            (a.zoom, a.hibernate_after_min, a.custom_css.as_deref()),
            (1.0, None, None)
        );
        let app = &p.settings.app;
        assert!(
            !app.notification_preview
                && app.launch_at_login
                && app.browser.as_deref() == Some("Firefox")
        );
        assert_eq!(app.link_target, protocol::LinkTarget::SystemBrowser);
        assert!(app.show_tray && app.reload_after_wake);
        assert!(p.custom_services.is_empty());
        assert!(
            d.path().join("accounts.json").exists(),
            "previous file must not be quarantined"
        );
    }

    fn uuid_like() -> uuid::Uuid {
        uuid::Uuid::from_u128(42)
    }

    #[test]
    fn round_trip_and_no_tmp() {
        let d = tempfile::tempdir().unwrap();
        let s = FsStore::new(d.path().join("nested"));
        s.save(&sample()).unwrap();
        s.save(&sample()).unwrap();
        assert_eq!(s.load().unwrap(), sample());
        assert!(!d.path().join("nested/accounts.json.tmp").exists());
    }

    #[test]
    fn missing_is_default() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(FsStore::new(d.path()).load().unwrap(), Persisted::default());
    }

    #[test]
    fn lists_all_accounts_only_with_an_intact_file() {
        let d = tempfile::tempdir().unwrap();
        let s = FsStore::new(d.path());
        assert!(!s.lists_all_accounts(), "no file yet");
        s.save(&sample()).unwrap();
        assert!(s.lists_all_accounts());
        fs::write(d.path().join("accounts.json.corrupt-1"), "{nope").unwrap();
        assert!(!s.lists_all_accounts(), "an older file was corrupt");
    }

    #[test]
    fn corrupt_is_renamed() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("accounts.json"), "{nope").unwrap();
        let s = FsStore::new(d.path());
        assert_eq!(s.load().unwrap(), Persisted::default());
        assert!(!d.path().join("accounts.json").exists());
        let found = fs::read_dir(d.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("accounts.json.corrupt-")
        });
        assert!(found);
        s.save(&sample()).unwrap();
        assert_eq!(s.load().unwrap(), sample());
    }
}
