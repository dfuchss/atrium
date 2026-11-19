//! What a sleep means for the loaded views, and views whose web content process died.
//!
//! The tick detects a sleep (a gap between clock readings); the views are handled only once the
//! user is back ([`Input::Resumed`]), not on a dark wake (Power Nap), which also leaves a gap. Each account follows
//! its own [`AfterWake`] mode, or the global one:
//! - `Check` watches the view: it is alive once it answers a poll after the wake, and reloaded
//!   if it stays silent for [`WAKE_GRACE_MS`]. Most services reconnect by themselves, and a
//!   reload costs some of them seconds of startup.
//! - `Reload` reloads it right away, `Never` leaves it alone.
//!
//! Only a non-empty poll result is an answer. The adapter reports every failed evaluation as an
//! empty result, whether the script threw or `WebKit` could not run it (a hung or terminated web
//! content process), and the poll script catches its own errors, so an empty result is taken
//! for a view that does not run JavaScript.

use protocol::AfterWake;
use uuid::Uuid;

use super::{Core, Effect};

/// Reloads of a view after its web process crashed, within [`CRASH_RELOAD_WINDOW_MS`].
pub(super) const MAX_CRASH_RELOADS: usize = 3;
/// The window, in ms of the tick clock, in which [`MAX_CRASH_RELOADS`] are allowed.
pub(super) const CRASH_RELOAD_WINDOW_MS: u64 = 60_000;

/// Time a checked view has after a wake to answer a poll before it is reloaded.
pub(super) const WAKE_GRACE_MS: u64 = 15_000;

impl Core {
    /// After a sleep, reloads or watches the loaded views as their mode says. The clock is read
    /// first: the wake can arrive before the first tick after it.
    pub(super) fn resumed(&mut self, now_ms: u64) -> Vec<Effect> {
        if !self.started {
            return vec![];
        }
        let mut effects: Vec<Effect> = self.read_clock(now_ms).into_iter().collect();
        if !std::mem::take(&mut self.slept) {
            return effects;
        }
        effects.push(Effect::Log("resumed after sleep".into()));
        for account_id in self.loaded_account_ids() {
            match self.after_wake(account_id) {
                AfterWake::Check => {
                    self.wake_checks.insert(account_id, now_ms);
                }
                AfterWake::Reload => effects.push(Effect::Reload(account_id)),
                AfterWake::Never => {}
            }
        }
        effects
    }

    /// The account's own mode, or the global one.
    fn after_wake(&self, account_id: Uuid) -> AfterWake {
        self.account(account_id)
            .and_then(|account| account.settings.after_wake)
            .unwrap_or(self.settings().after_wake)
    }

    /// Ends the check of a watched account that answered a poll (`raw` is its result).
    pub(super) fn wake_check_answered(&mut self, account_id: Uuid, raw: &str) -> Option<Effect> {
        if raw.is_empty() {
            return None;
        }
        let woke_ms = self.wake_checks.remove(&account_id)?;
        let after_ms = self.last_tick_ms.unwrap_or(woke_ms).saturating_sub(woke_ms);
        Some(Effect::Log(format!(
            "[{account_id}] alive after wake ({after_ms} ms)"
        )))
    }

    /// Reloads the watched accounts that did not answer within [`WAKE_GRACE_MS`], in sidebar
    /// order.
    pub(super) fn reload_silent_after_wake(&mut self, now_ms: u64) -> Vec<Effect> {
        let silent: Vec<(Uuid, u64)> = self
            .persisted
            .accounts
            .iter()
            .filter_map(|account| {
                let woke_ms = self.wake_checks.get(&account.id)?;
                Some((account.id, now_ms.saturating_sub(*woke_ms)))
            })
            .filter(|&(_, since_wake_ms)| since_wake_ms >= WAKE_GRACE_MS)
            .collect();
        let mut effects = vec![];
        for (account_id, since_wake_ms) in silent {
            self.stop_wake_check(account_id);
            effects.extend(logged_reload(
                account_id,
                &format!("no answer {} s after wake", since_wake_ms / 1000),
            ));
        }
        effects
    }

    /// Stops watching the account (its view is gone or reloads anyway).
    fn stop_wake_check(&mut self, account_id: Uuid) {
        self.wake_checks.remove(&account_id);
    }

    /// A view whose web content process died shows a blank page until it is reloaded. A page
    /// that crashes it again and again is reloaded at most [`MAX_CRASH_RELOADS`] times within
    /// [`CRASH_RELOAD_WINDOW_MS`] (tick clock), then left alone until the user reloads it.
    pub(super) fn web_process_terminated(&mut self, account_id: Uuid) -> Vec<Effect> {
        if !self.is_loaded(account_id) {
            return vec![];
        }
        self.stop_wake_check(account_id);
        // The dead process never reports the end of its capture.
        let mut effects = self.capture_changed(account_id, None, false);
        let now_ms = self.last_tick_ms.unwrap_or(0);
        if self.crash_reload_limit.allow(account_id, now_ms) {
            self.crash_reloads_stopped.remove(&account_id);
            effects.extend(logged_reload(account_id, "web process terminated"));
        } else if self.crash_reloads_stopped.insert(account_id) {
            effects.push(Effect::Log(format!(
                "[{account_id}] web process keeps crashing -> not reloading"
            )));
        }
        effects
    }

    /// A reload by the user or a new view starts the account afresh: its wake check ends and
    /// its crashes are reloaded again from zero.
    pub(super) fn start_afresh(&mut self, account_id: Uuid) {
        self.stop_wake_check(account_id);
        self.crash_reload_limit.reset(&account_id);
        self.crash_reloads_stopped.remove(&account_id);
    }
}

/// Reloads the account's view, logging `reason`.
fn logged_reload(account_id: Uuid, reason: &str) -> [Effect; 2] {
    [
        Effect::Log(format!("[{account_id}] {reason} -> reload")),
        Effect::Reload(account_id),
    ]
}
