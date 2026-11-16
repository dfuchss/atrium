//! The one-second tick: sleep detection, hibernation of idle accounts and unread polling.
//! Views are reloaded after a sleep only once the displays wake, not on a dark wake (Power Nap),
//! which also leaves a gap between ticks.

use uuid::Uuid;

use super::{Core, Effect, scripts};

/// A gap between two clock readings longer than this means the Mac was asleep.
pub const SLEEP_GAP_MS: u64 = 30_000;

const MS_PER_MINUTE: u64 = 60_000;

impl Core {
    pub(super) fn tick(&mut self, now_ms: u64) -> Vec<Effect> {
        if !self.started {
            return vec![];
        }
        let mut effects: Vec<Effect> = self.read_clock(now_ms).into_iter().collect();
        if let Some(active) = self.active {
            self.last_activity_ms.insert(active, now_ms);
        }
        let hibernated = self.hibernate_idle_accounts(now_ms);
        let any_hibernated = !hibernated.is_empty();
        effects.extend(hibernated);
        effects.extend(self.poll_effects());
        if any_hibernated {
            effects.push(self.emit());
        }
        effects
    }

    /// After a sleep, reloads the loaded views (their connections are stale) if the setting
    /// says so. The clock is read first: the wake can arrive before the first tick after it.
    pub(super) fn screens_woke(&mut self, now_ms: u64) -> Vec<Effect> {
        if !self.started {
            return vec![];
        }
        let mut effects: Vec<Effect> = self.read_clock(now_ms).into_iter().collect();
        if !std::mem::take(&mut self.slept) {
            return effects;
        }
        effects.push(Effect::Log("screens woke after sleep".into()));
        if self.settings().reload_after_wake {
            effects.extend(self.loaded_account_ids().into_iter().map(Effect::Reload));
        }
        effects
    }

    /// Records `now_ms` as the latest clock reading; a gap longer than [`SLEEP_GAP_MS`] since
    /// the previous one marks a sleep, logged here.
    fn read_clock(&mut self, now_ms: u64) -> Option<Effect> {
        let since_last_ms = self
            .last_tick_ms
            .map_or(0, |last| now_ms.saturating_sub(last));
        self.last_tick_ms = Some(now_ms);
        if since_last_ms <= SLEEP_GAP_MS {
            return None;
        }
        self.slept = true;
        Some(Effect::Log(format!(
            "sleep detected ({} s since the last tick)",
            since_last_ms / 1000
        )))
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

    /// One poll per loaded view, which drains its notifications, preceded by the evaluation
    /// of the service's unread expression if it has one.
    fn poll_effects(&self) -> Vec<Effect> {
        let mut effects = vec![];
        for account in &self.persisted.accounts {
            let Some(service) = self.catalog.get(&account.service) else {
                continue;
            };
            if !self.is_loaded(account.id) {
                continue;
            }
            if let Some(expression) = &service.unread.poll {
                effects.push(Effect::Eval {
                    account: account.id,
                    script: scripts::unread_script(expression),
                });
            }
            effects.push(Effect::Poll {
                account: account.id,
                script: scripts::POLL_SCRIPT.into(),
            });
        }
        effects
    }
}
