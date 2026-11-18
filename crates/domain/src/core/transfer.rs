//! Export and import of the settings, the added services and the custom services.

use protocol::{AccountSettings, AppSettings, CustomService};
use serde::{Deserialize, Serialize};

use super::{Core, Effect, log_effects};
use crate::model::Account;

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
    /// content of `json`. The file is validated completely first: a file that cannot be read,
    /// with an invalid custom service or with invalid account settings changes nothing.
    /// Accounts of unknown services are skipped (logged).
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
        let custom = match valid_custom_services(&file.custom_services) {
            Ok(custom) => custom,
            Err(error) => return self.rejected(format!("import settings: {error}")),
        };
        let problems = self.registry.rebuild(&custom);
        let mut skipped = vec![];
        let accounts = match self.imported_accounts(file.accounts, &mut skipped) {
            Ok(accounts) if problems.is_empty() => accounts,
            result => {
                self.registry.rebuild(&self.persisted.custom_services);
                let error = problems
                    .into_iter()
                    .next()
                    .or_else(|| result.err())
                    .unwrap_or_default();
                return self.rejected(format!("import settings: {error}"));
            }
        };
        self.replace_all(file.settings, custom, accounts, skipped)
    }

    /// The accounts of the file with normalised settings, in the catalog that is already
    /// rebuilt; accounts of unknown services are left out (logged to `skipped`).
    fn imported_accounts(
        &mut self,
        entries: Vec<FileAccount>,
        skipped: &mut Vec<String>,
    ) -> Result<Vec<Account>, String> {
        let mut accounts = vec![];
        for entry in entries {
            if self.registry.get(&entry.service).is_none() {
                skipped.push(format!(
                    "import settings: skipped account of unknown service {}",
                    entry.service
                ));
                continue;
            }
            let mut account = Account::new(
                (self.new_account_id)(),
                &entry.service,
                &entry.settings.label,
                None,
            );
            let (settings, problems) =
                self.normalized_account_settings(&account, entry.settings)
                    .map_err(|error| format!("account of {}: {error}", entry.service))?;
            skipped.extend(
                problems
                    .into_iter()
                    .map(|problem| format!("import settings: {}: {problem}", entry.service)),
            );
            account.apply_settings(settings);
            accounts.push(account);
        }
        Ok(accounts)
    }

    /// Applies the validated import in one step: persists and emits only the final state, and
    /// creates the views of the new accounts with their final settings.
    fn replace_all(
        &mut self,
        settings: AppSettings,
        custom: Vec<CustomService>,
        accounts: Vec<Account>,
        skipped: Vec<String>,
    ) -> Vec<Effect> {
        let mut effects = log_effects(skipped);
        for account in std::mem::take(&mut self.persisted.accounts) {
            self.unread.forget(account.id);
            self.last_activity_ms.remove(&account.id);
            self.favicon_attempts.remove(&account.id);
            effects.extend(self.unload_view(account.id));
            effects.push(Effect::DeleteProfile {
                store_id: account.store_id(),
            });
        }
        let imported = accounts.len();
        self.persisted.accounts = accounts;
        self.persisted.custom_services = custom;
        self.active = self.nearest_enabled_account(0, None);
        let mut applied = self.update_settings(settings);
        applied.retain(|effect| !matches!(effect, Effect::Persist(_) | Effect::EmitSnapshot(_)));
        effects.extend(applied);
        if self.started {
            let (to_load, _asleep) = self.split_startup_accounts();
            for account_id in to_load {
                let visible = self.is_visible(account_id);
                effects.extend(self.load_view(account_id, visible));
            }
            effects.extend(self.fetch_missing_simple_icons());
        }
        effects.push(Effect::Log(format!(
            "imported settings: {imported} account(s)"
        )));
        effects.push(self.persist());
        effects.extend(self.dock_badge());
        effects.push(self.emit());
        effects
    }
}

/// The custom services after validation; an invalid or duplicate one is an error.
fn valid_custom_services(services: &[CustomService]) -> Result<Vec<CustomService>, String> {
    let mut custom: Vec<CustomService> = vec![];
    for service in services {
        let service = crate::catalog::normalize_custom(service)?;
        if service.id.is_empty() || custom.iter().any(|c| c.id == service.id) {
            return Err(format!(
                "custom service {:?} has a missing or duplicate id",
                service.name
            ));
        }
        custom.push(service);
    }
    Ok(custom)
}
