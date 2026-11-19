//! The audio of the views: muted while Do Not Disturb is on, except in a view that captures
//! the microphone or camera, so a call stays audible.

use uuid::Uuid;

use super::{Core, Effect};

impl Core {
    /// Whether the audio of `account`'s view is muted now.
    pub(super) fn audio_muted(&self, account: Uuid) -> bool {
        self.settings().dnd && !self.capturing.contains(&account)
    }

    /// `SetAudioMuted` for the loaded views whose mute follows a Do Not Disturb change: those
    /// that capture keep their audio either way.
    pub(super) fn audio_muted_effects(&self) -> Vec<Effect> {
        self.loaded_account_ids()
            .into_iter()
            .filter(|account| !self.capturing.contains(account))
            .map(|account| Effect::SetAudioMuted {
                account,
                muted: self.audio_muted(account),
            })
            .collect()
    }

    /// Records whether the account's view captures; under Do Not Disturb its audio follows.
    /// Ignored for an account without a view (a late report of a destroyed one).
    pub(super) fn capture_changed(&mut self, account: Uuid, active: bool) -> Vec<Effect> {
        if !self.is_loaded(account) {
            return vec![];
        }
        let changed = if active {
            self.capturing.insert(account)
        } else {
            self.capturing.remove(&account)
        };
        if !changed {
            return vec![];
        }
        let state = if active { "started" } else { "stopped" };
        let mut effects = vec![Effect::Log(format!("[{account}] capture {state}"))];
        if self.settings().dnd {
            effects.push(Effect::SetAudioMuted {
                account,
                muted: self.audio_muted(account),
            });
        }
        effects
    }
}
