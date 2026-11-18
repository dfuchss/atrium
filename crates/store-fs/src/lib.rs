//! File-based [`domain::ports::Store`]: one JSON file (`ACCOUNTS_FILE`) in a directory, and the
//! user's service definition files ([`services`]).

pub mod services;

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use domain::Persisted;
use domain::ports::{PortError, PortResult, Store};

pub struct FsStore {
    dir: PathBuf,
    /// What [`Store::load`] set aside as corrupt, until [`FsStore::take_quarantined`].
    quarantined: Mutex<Option<String>>,
}

impl FsStore {
    pub fn new(dir: impl Into<PathBuf>) -> FsStore {
        FsStore {
            dir: dir.into(),
            quarantined: Mutex::default(),
        }
    }

    fn file(&self) -> PathBuf {
        self.dir.join(ACCOUNTS_FILE)
    }

    /// Why and where `load` set the accounts file aside as corrupt (it then loads as empty), once.
    pub fn take_quarantined(&self) -> Option<String> {
        fs_util::lock_ignoring_poison(&self.quarantined).take()
    }

    /// True if the accounts file exists and no earlier version was set aside as corrupt: only
    /// then does it name every account whose web data may still be in use.
    pub fn lists_all_accounts(&self) -> bool {
        let quarantined = fs::read_dir(&self.dir).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&corrupt_file_prefix())
            })
        });
        self.file().is_file() && !quarantined
    }
}

const ACCOUNTS_FILE: &str = "accounts.json";

/// The file names the user's accounts and workspaces: readable by the user only.
const ACCOUNTS_FILE_MODE: u32 = 0o600;

/// Name prefix of a corrupt [`ACCOUNTS_FILE`] kept for inspection.
fn corrupt_file_prefix() -> String {
    format!("{ACCOUNTS_FILE}.corrupt-")
}

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
            Err(error) => return Err(port_error(&format!("read {ACCOUNTS_FILE}"), error)),
        };
        let parse_error = match serde_json::from_slice(&bytes) {
            Ok(persisted) => return Ok(persisted),
            Err(error) => error,
        };
        // Keep the unreadable file for inspection and start empty.
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since_epoch| since_epoch.as_secs());
        let target = self.dir.join(format!("{}{seconds}", corrupt_file_prefix()));
        fs::rename(&path, &target).map_err(|error| {
            port_error(
                &format!("quarantine corrupt {ACCOUNTS_FILE} ({parse_error})"),
                error,
            )
        })?;
        *fs_util::lock_ignoring_poison(&self.quarantined) = Some(format!(
            "corrupt {ACCOUNTS_FILE} ({parse_error}) moved to {}",
            target.display()
        ));
        Ok(Persisted::default())
    }

    fn save(&self, data: &Persisted) -> PortResult {
        let json =
            serde_json::to_vec_pretty(data).map_err(|error| port_error("serialize", error))?;
        fs_util::write_atomically(&self.file(), &json, ACCOUNTS_FILE_MODE)
            .map_err(|error| port_error(&format!("write {ACCOUNTS_FILE}"), error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Account, Settings};
    use protocol::AppSettings;

    fn sample() -> Persisted {
        let id = uuid_like();
        let mut account = Account::new(id, "slack", "KIT", Some("x".into()));
        account.settings.muted = true;
        account.settings.in_dock_badge = false;
        account.settings.user_agent = Some("chrome".into());
        Persisted {
            accounts: vec![account],
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
        assert_eq!(fs::read_dir(d.path().join("nested")).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn saved_file_is_owner_only() {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt as _;

        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join(ACCOUNTS_FILE), "readable").unwrap();
        fs::set_permissions(d.path().join(ACCOUNTS_FILE), Permissions::from_mode(0o644)).unwrap();
        FsStore::new(d.path()).save(&sample()).unwrap();
        let mode = fs::metadata(d.path().join(ACCOUNTS_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, ACCOUNTS_FILE_MODE);
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
        fs::write(
            d.path().join(format!("{}1", corrupt_file_prefix())),
            "{nope",
        )
        .unwrap();
        assert!(!s.lists_all_accounts(), "an older file was corrupt");
    }

    #[test]
    fn corrupt_is_renamed() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join(ACCOUNTS_FILE), "{nope").unwrap();
        let s = FsStore::new(d.path());
        assert_eq!(s.load().unwrap(), Persisted::default());
        assert!(
            s.take_quarantined()
                .is_some_and(|note| note.contains("corrupt"))
        );
        assert_eq!(s.take_quarantined(), None);
        assert!(!d.path().join(ACCOUNTS_FILE).exists());
        let found = fs::read_dir(d.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(&corrupt_file_prefix())
        });
        assert!(found);
        s.save(&sample()).unwrap();
        assert_eq!(s.load().unwrap(), sample());
    }
}
