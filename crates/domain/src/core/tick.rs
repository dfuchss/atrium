//! The one-second tick: wake detection, hibernation of idle accounts and unread polling.

use uuid::Uuid;

use super::{Core, Effect, scripts};

/// A gap between two ticks longer than this means the Mac was asleep.
pub const WAKE_GAP_MS: u64 = 30_000;

const MS_PER_MINUTE: u64 = 60_000;

impl Core {
    pub(super) fn tick(&mut self, now_ms: u64) -> Vec<Effect> {
        if !self.started {
            return vec![];
        }
        let mut effects = vec![];
        let since_last_tick_ms = self
            .last_tick_ms
            .map_or(0, |last| now_ms.saturating_sub(last));
        let woke = since_last_tick_ms > WAKE_GAP_MS;
        if woke {
            effects.push(Effect::Log(format!(
                "wake detected ({} s since the last tick)",
                since_last_tick_ms / 1000
            )));
        }
        self.last_tick_ms = Some(now_ms);
        if let Some(active) = self.active {
            self.last_activity_ms.insert(active, now_ms);
        }
        let hibernated = self.hibernate_idle_accounts(now_ms);
        let any_hibernated = !hibernated.is_empty();
        effects.extend(hibernated);
        if woke && self.settings().reload_after_wake {
            effects.extend(self.loaded_account_ids().into_iter().map(Effect::Reload));
        }
        effects.extend(self.poll_effects());
        if any_hibernated {
            effects.push(self.emit());
        }
        effects
    }

    /// Unloads background accounts that were idle longer than their hibernation timeout.
    fn hibernate_idle_accounts(&mut self, now_ms: u64) -> Vec<Effect> {
        let mut effects = vec![];
        for (account_id, minutes) in self.idle_accounts(now_ms) {
            effects.extend(self.unload_view(account_id));
            effects.push(Effect::Log(format!(
                "hibernated account {account_id} after {minutes} min in the background"
            )));
        }
        effects
    }

    /// Loaded background accounts past their timeout, with the timeout in minutes.
    fn idle_accounts(&mut self, now_ms: u64) -> Vec<(Uuid, u32)> {
        let global_minutes = self.settings().hibernate_after_min;
        let mut idle = vec![];
        for account in &self.persisted.accounts {
            if Some(account.id) == self.active || !self.loaded_views.contains(&account.id) {
                continue;
            }
            // Tracked even without a timeout, so a later setting counts from the last activity.
            let last_activity = *self.last_activity_ms.entry(account.id).or_insert(now_ms);
            // The account's own timeout wins; 0, or none at all, means never.
            let Some(minutes) = account
                .hibernate_after_min
                .or(global_minutes)
                .filter(|&minutes| minutes > 0)
            else {
                continue;
            };
            if now_ms.saturating_sub(last_activity) >= u64::from(minutes) * MS_PER_MINUTE {
                idle.push((account.id, minutes));
            }
        }
        idle
    }

    /// One poll per loaded view: drains its notifications and reads the unread expression.
    fn poll_effects(&self) -> Vec<Effect> {
        self.persisted
            .accounts
            .iter()
            .filter(|account| self.is_loaded(account.id))
            .filter_map(|account| {
                let service = self.catalog.get(&account.service)?;
                Some(Effect::Poll {
                    account: account.id,
                    script: scripts::poll_script(service.unread.poll.as_deref()),
                })
            })
            .collect()
    }
}
