//! The audio of the views: muted while Do Not Disturb is on, except for an account whose view
//! or one of its in-app windows captures the microphone or camera, so a call stays audible.

use uuid::Uuid;

use super::{Core, Effect};

impl Core {
    /// Whether the audio of `account`'s view and windows is muted now.
    pub(super) fn audio_muted(&self, account: Uuid) -> bool {
        self.settings().dnd && !self.capturing.contains_key(&account)
    }

    /// `SetAudioMuted` for the loaded views if Do Not Disturb changed from `was`; capturing
    /// accounts keep their audio either way.
    pub(super) fn dnd_changed(&self, was: bool) -> Vec<Effect> {
        let muted = self.settings().dnd;
        if muted == was {
            return vec![];
        }
        self.loaded_account_ids()
            .into_iter()
            .filter(|account| !self.capturing.contains_key(account))
            .map(|account| Effect::SetAudioMuted { account, muted })
            .collect()
    }

    /// Records whether `window` (None: the view) of `account` captures; under Do Not Disturb
    /// the account's audio follows. Ignored for an account without a view (a late report).
    pub(super) fn capture_changed(
        &mut self,
        account: Uuid,
        window: Option<String>,
        active: bool,
    ) -> Vec<Effect> {
        if !self.is_loaded(account) {
            return vec![];
        }
        let was_capturing = self.capturing.contains_key(&account);
        let sources = self.capturing.entry(account).or_default();
        let source = window.as_deref().unwrap_or("view").to_string();
        let changed = if active {
            sources.insert(window)
        } else {
            sources.remove(&window)
        };
        if sources.is_empty() {
            self.capturing.remove(&account);
        }
        if !changed {
            return vec![];
        }
        let state = if active { "started" } else { "stopped" };
        let mut effects = vec![Effect::Log(format!(
            "[{account}] capture {state} ({source})"
        ))];
        if self.settings().dnd && was_capturing != self.capturing.contains_key(&account) {
            effects.push(Effect::SetAudioMuted {
                account,
                muted: self.audio_muted(account),
            });
        }
        effects
    }
}
