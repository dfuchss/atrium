//! Account lifecycle: start, selection, adding, removing, reordering and hibernating.

use uuid::Uuid;

use super::layout::WindowSize;
use super::{Core, Effect, log_effects};
use crate::catalog::ServiceDef;
use crate::model::Account;

impl Core {
    /// Creates the views of all enabled accounts; the first input after the window exists.
    pub(super) fn started(&mut self, window: WindowSize) -> Vec<Effect> {
        self.window = window;
        self.started = true;
        self.last_badge = None;
        let active_is_enabled = self
            .active
            .and_then(|id| self.account(id))
            .is_some_and(|account| account.enabled);
        if !active_is_enabled {
            self.active = self.nearest_enabled_account(0, None);
        }
        self.persisted.settings.active = self.active;
        let mut effects = vec![
            Effect::ApplySystemSettings(self.settings().clone()),
            Effect::SetUiBounds(self.sidebar_bounds()),
            Effect::ApplyTheme(self.settings().theme),
        ];
        effects.extend(log_effects(std::mem::take(&mut self.startup_log)));
        let (to_load, asleep) = self.split_startup_accounts();
        for account_id in asleep {
            effects.push(Effect::Log(format!("[{account_id}] starts asleep")));
        }
        for account_id in to_load {
            effects.extend(self.load_view(account_id, Some(account_id) == self.active));
        }
        effects.extend(self.fetch_missing_simple_icons());
        effects.extend(self.dock_badge());
        effects.push(self.emit());
        effects
    }

    /// The enabled accounts to load at startup and those that start asleep. The active
    /// account is always loaded: the user is looking at it.
    fn split_startup_accounts(&self) -> (Vec<Uuid>, Vec<Uuid>) {
        let (asleep, to_load): (Vec<&Account>, Vec<&Account>) = self
            .persisted
            .accounts
            .iter()
            .filter(|account| account.enabled)
            .partition(|account| account.start_asleep && Some(account.id) != self.active);
        let ids = |accounts: Vec<&Account>| accounts.iter().map(|account| account.id).collect();
        (ids(to_load), ids(asleep))
    }

    /// Selecting a disabled account is refused (logged): it has no view to show, and the
    /// UI enables it through the account settings.
    pub(super) fn select(&mut self, account_id: Uuid) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        if !account.enabled {
            return vec![
                Effect::Log(format!("select: account {account_id} is disabled")),
                self.emit(),
            ];
        }
        let mut effects = vec![];
        if self.dialog_open {
            self.dialog_open = false;
            effects.push(Effect::SetUiBounds(self.sidebar_bounds()));
        }
        if let Some(previous) = self.active.filter(|&previous| previous != account_id) {
            if self.is_loaded(previous) {
                effects.push(Effect::Hide(previous));
            }
            self.record_activity(previous);
        }
        self.active = Some(account_id);
        self.record_activity(account_id);
        effects.extend(self.show_active());
        effects.push(self.persist());
        effects.push(self.emit());
        effects
    }

    pub(super) fn add_account(
        &mut self,
        service_id: &str,
        label: &str,
        instance: Option<String>,
    ) -> Vec<Effect> {
        let instance = normalized_instance(instance);
        let Some(service) = self.catalog.get(service_id) else {
            return vec![
                Effect::Log(format!("add account: unknown service {service_id}")),
                self.emit(),
            ];
        };
        if let Err(error) = service.resolve_url(instance.as_deref()) {
            return vec![Effect::Log(format!("add account: {error}")), self.emit()];
        }
        let label = account_label(label, service);
        let account = Account::new((self.new_account_id)(), service_id, label, instance);
        let account_id = account.id;
        self.persisted.accounts.push(account);
        let mut effects = match (self.started, self.dialog_open) {
            (false, _) => vec![],
            (true, true) => self.load_added_behind_dialog(account_id),
            (true, false) => self.load_added_and_show(account_id),
        };
        effects.push(self.persist());
        effects.push(self.emit());
        effects
    }

    /// Added from the settings dialog: keep the dialog open (the user may add more) and load
    /// the view hidden. It only becomes active if nothing else is; closing the dialog shows it.
    fn load_added_behind_dialog(&mut self, account_id: Uuid) -> Vec<Effect> {
        let effects = self.load_view(account_id, false);
        let has_active = self.active.is_some_and(|active| self.has_account(active));
        if !has_active && self.is_loaded(account_id) {
            self.active = Some(account_id);
            self.record_activity(account_id);
        }
        effects
    }

    /// Loads the new account's view and makes it the active one.
    fn load_added_and_show(&mut self, account_id: Uuid) -> Vec<Effect> {
        let previous = self.active.filter(|&previous| self.is_loaded(previous));
        let created = self.load_view(account_id, true);
        let mut effects = vec![];
        if self.is_loaded(account_id) {
            if let Some(previous) = previous {
                effects.push(Effect::Hide(previous));
                self.record_activity(previous);
            }
            self.active = Some(account_id);
            self.record_activity(account_id);
        }
        effects.extend(created);
        effects
    }

    pub(super) fn remove_account(&mut self, account_id: Uuid) -> Vec<Effect> {
        let Some(index) = self.account_index(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        let account = self.persisted.accounts.remove(index);
        self.unread.forget(account_id);
        self.last_activity_ms.remove(&account_id);
        self.favicon_attempts.remove(&account_id);
        let mut effects = self.unload_view(account_id);
        effects.push(Effect::DeleteProfile {
            store_id: account.store_id(),
        });
        if self.active == Some(account_id) {
            self.active = self.nearest_enabled_account(index, None);
            effects.extend(self.show_active());
        }
        effects.push(self.persist());
        effects.extend(self.dock_badge());
        effects.push(self.emit());
        effects
    }

    pub(super) fn move_account(&mut self, account_id: Uuid, delta: i32) -> Vec<Effect> {
        let Some(index) = self.account_index(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        let accounts = &mut self.persisted.accounts;
        let target = moved_index(index, delta, accounts.len());
        let account = accounts.remove(index);
        accounts.insert(target, account);
        vec![self.persist(), self.emit()]
    }

    pub(super) fn set_muted(&mut self, account_id: Uuid, muted: bool) -> Vec<Effect> {
        let Some(account) = self.account_mut(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        account.muted = muted;
        let mut effects = vec![self.persist()];
        effects.extend(self.dock_badge());
        effects.push(self.emit());
        effects
    }

    /// Unloads a background account's view; it is loaded again when selected.
    pub(super) fn hibernate(&mut self, account_id: Uuid) -> Vec<Effect> {
        if !self.has_account(account_id) {
            return vec![Self::unknown(account_id)];
        }
        if self.active == Some(account_id) {
            return vec![Effect::Log(format!(
                "hibernate: not unloading the active account {account_id}"
            ))];
        }
        let mut effects = self.unload_view(account_id);
        if !effects.is_empty() {
            effects.push(Effect::Log(format!("hibernated account {account_id}")));
        }
        effects.push(self.emit());
        effects
    }

    /// Nearest enabled account to position `index` (at or after it first, then before),
    /// skipping `except`.
    pub(super) fn nearest_enabled_account(
        &self,
        index: usize,
        except: Option<Uuid>,
    ) -> Option<Uuid> {
        let accounts = &self.persisted.accounts;
        let candidate = |account: &&Account| account.enabled && Some(account.id) != except;
        accounts
            .iter()
            .skip(index)
            .find(candidate)
            .or_else(|| accounts.iter().take(index).rev().find(candidate))
            .map(|account| account.id)
    }

    /// Notes the account was just shown or left; hibernation counts from here.
    pub(super) fn record_activity(&mut self, account_id: Uuid) {
        if let Some(now) = self.last_tick_ms {
            self.last_activity_ms.insert(account_id, now);
        }
    }
}

/// The trimmed label, or the service name if it is empty.
pub(super) fn account_label(label: &str, service: &ServiceDef) -> String {
    let label = label.trim();
    if label.is_empty() {
        service.name.clone()
    } else {
        label.to_string()
    }
}

/// Normalised instance; empty means none.
pub(super) fn normalized_instance(instance: Option<String>) -> Option<String> {
    instance
        .map(|instance| crate::catalog::normalize_instance(&instance))
        .filter(|instance| !instance.is_empty())
}

/// Index `delta` positions away from `index`, clamped to a list of `len` items.
fn moved_index(index: usize, delta: i32, len: usize) -> usize {
    let steps = usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX);
    if delta < 0 {
        index.saturating_sub(steps)
    } else {
        index.saturating_add(steps).min(len.saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::moved_index;

    #[test]
    fn moved_index_clamps_to_the_list() {
        assert_eq!(moved_index(1, -1, 3), 0);
        assert_eq!(moved_index(1, -5, 3), 0);
        assert_eq!(moved_index(1, 1, 3), 2);
        assert_eq!(moved_index(1, i32::MAX, 3), 2);
        assert_eq!(moved_index(2, i32::MIN, 3), 0);
        assert_eq!(moved_index(0, 0, 1), 0);
    }
}
