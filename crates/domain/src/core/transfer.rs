//! Export and import of the settings, the added services and the custom services.

use protocol::{AccountSettings, AppSettings, CustomService};
use serde::{Deserialize, Serialize};

use super::{Core, Effect, log_effects};

const FORMAT: &str = "atrium-settings";
const VERSION: u32 = 1;
/// Larger files are refused (custom icons are stored in the file as data URLs).
const MAX_IMPORT_BYTES: usize = 32 * 1024 * 1024;

/// The export file. Accounts are stored without their id, login and caches.
#[derive(Serialize, Deserialize)]
struct SettingsFile {
    format: String,
    version: u32,
    #[serde(default)]
    settings: AppSettings,
    #[serde(default)]
    custom_services: Vec<CustomService>,
    #[serde(default)]
    accounts: Vec<FileAccount>,
}

#[derive(Serialize, Deserialize)]
struct FileAccount {
    service: String,
    #[serde(flatten)]
    settings: AccountSettings,
}

impl Core {
    pub(super) fn export_settings(&self) -> Vec<Effect> {
        let file = SettingsFile {
            format: FORMAT.into(),
            version: VERSION,
            settings: self.settings().clone(),
            custom_services: self.persisted.custom_services.clone(),
            accounts: self
                .persisted
                .accounts
                .iter()
                .map(|account| FileAccount {
                    service: account.service.clone(),
                    settings: account.settings.clone(),
                })
                .collect(),
        };
        match serde_json::to_string_pretty(&file) {
            Ok(json) => vec![Effect::ExportSettings { json }],
            Err(error) => self.rejected(format!("export settings: {error}")),
        }
    }

    /// Replaces the settings, the custom services and all accounts (with their logins) by the
    /// content of `json`. A file that cannot be read changes nothing; accounts of unknown
    /// services or with invalid settings are skipped (logged).
    pub(super) fn import_settings(&mut self, json: &str) -> Vec<Effect> {
        if json.len() > MAX_IMPORT_BYTES {
            return self.rejected("import settings: file too large".into());
        }
        let file: SettingsFile = match serde_json::from_str(json) {
            Ok(file) => file,
            Err(error) => return self.rejected(format!("import settings: {error}")),
        };
        if file.format != FORMAT || file.version > VERSION {
            return self.rejected(format!(
                "import settings: not a settings file of this version ({:?}, version {})",
                file.format, file.version
            ));
        }
        let mut effects = vec![];
        let old: Vec<_> = self.persisted.accounts.iter().map(|a| a.id).collect();
        for account_id in old {
            effects.extend(self.remove_account(account_id));
        }
        let custom = valid_custom_services(&file.custom_services, &mut effects);
        self.persisted.custom_services = custom;
        effects.extend(log_effects(self.rebuild_catalog()));
        effects.extend(self.update_settings(file.settings));
        let mut imported = 0;
        for entry in file.accounts {
            let before = self.persisted.accounts.len();
            effects.extend(self.add_account(
                &entry.service,
                &entry.settings.label,
                entry.settings.instance.as_deref(),
            ));
            let Some(account_id) = self
                .persisted
                .accounts
                .get(before)
                .map(|account| account.id)
            else {
                continue;
            };
            effects.extend(self.update_account(account_id, entry.settings));
            imported += 1;
        }
        effects.push(Effect::Log(format!(
            "imported settings: {imported} account(s)"
        )));
        effects.push(self.persist());
        effects.push(self.emit());
        effects
    }
}

/// The custom services that pass validation and have a unique id; the others are logged.
fn valid_custom_services(
    services: &[CustomService],
    effects: &mut Vec<Effect>,
) -> Vec<CustomService> {
    let mut custom: Vec<CustomService> = vec![];
    for service in services {
        match crate::catalog::normalize_custom(service) {
            Ok(service) if !service.id.is_empty() && custom.iter().all(|c| c.id != service.id) => {
                custom.push(service);
            }
            Ok(service) => effects.push(Effect::Log(format!(
                "import settings: skipped custom service {:?} (missing or duplicate id)",
                service.name
            ))),
            Err(error) => effects.push(Effect::Log(format!("import settings: {error}"))),
        }
    }
    custom
}
