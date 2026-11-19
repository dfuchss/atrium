//! The audio of an account's in-app windows in Do Not Disturb. The core decides for the
//! service view ([`domain::Effect::SetAudioMuted`]); its windows follow it, except a window
//! whose own page captures the microphone or camera (a call in a popup), which keeps its audio.
//! The service view's own capture goes to the core ([`domain::Input::CaptureChanged`]).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use fs_util::lock_ignoring_poison;
use tauri::{AppHandle, Manager, Runtime, WebviewWindow, WindowEvent};
use uuid::Uuid;

use crate::labels::popup_label_prefix;
use crate::{Logger, native};

/// What decides the mute of the in-app windows, written by the actor (`set_audio_muted`) and
/// by the windows' capture observers (main thread).
#[derive(Debug, Default)]
pub(crate) struct WindowMutes {
    /// Accounts whose service view is muted.
    muted_accounts: HashSet<Uuid>,
    /// Labels of the in-app windows that capture the microphone or camera.
    capturing_windows: HashSet<String>,
}

pub(crate) type SharedWindowMutes = Arc<Mutex<WindowMutes>>;

impl WindowMutes {
    pub(crate) fn set_account_muted(&mut self, account: Uuid, muted: bool) {
        if muted {
            self.muted_accounts.insert(account);
        } else {
            self.muted_accounts.remove(&account);
        }
    }

    /// Forgets `account` and its windows (its view is destroyed).
    pub(crate) fn forget_account(&mut self, account: Uuid) {
        self.muted_accounts.remove(&account);
        let prefix = popup_label_prefix(account);
        self.capturing_windows
            .retain(|label| !label.starts_with(&prefix));
    }

    fn set_window_capturing(&mut self, label: &str, capturing: bool) {
        if capturing {
            self.capturing_windows.insert(label.to_string());
        } else {
            self.capturing_windows.remove(label);
        }
    }

    /// Whether `account`'s in-app window `label` is muted.
    pub(crate) fn window_muted(&self, account: Uuid, label: &str) -> bool {
        self.muted_accounts.contains(&account) && !self.capturing_windows.contains(label)
    }
}

/// Gives `account`'s new in-app `window` its mute and keeps it up to date with the window's
/// capture. Read after the window exists, so a change meanwhile reaches it too:
/// `set_audio_muted` lists the windows after updating `mutes`.
pub(crate) fn watch_window<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
    account: Uuid,
    mutes: &SharedWindowMutes,
    logger: &Logger,
) {
    let label = window.label().to_string();
    if lock_ignoring_poison(mutes).window_muted(account, &label) {
        apply(window, true, logger);
    }
    let (capture_app, capture_mutes, capture_logger) = (app.clone(), mutes.clone(), logger.clone());
    let capture_label = label.clone();
    let on_capture = Box::new(move |capturing: bool| {
        let window_muted = {
            let mut mutes = lock_ignoring_poison(&capture_mutes);
            mutes.set_window_capturing(&capture_label, capturing);
            mutes.window_muted(account, &capture_label)
        };
        if let Some(window) = capture_app.get_webview_window(&capture_label) {
            apply(&window, window_muted, &capture_logger);
        }
    });
    native::observe_capture(window.as_ref(), on_capture, logger);
    let closed_mutes = mutes.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            lock_ignoring_poison(&closed_mutes).set_window_capturing(&label, false);
        }
    });
}

/// Mutes or unmutes the in-app `window`; a failure is logged.
pub(crate) fn apply<R: Runtime>(window: &WebviewWindow<R>, muted: bool, logger: &Logger) {
    if let Err(error) = native::set_audio_muted(window.as_ref(), muted, logger) {
        logger.log(&format!("[{}] cannot mute audio: {error}", window.label()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::popup_label;

    #[test]
    fn a_window_follows_its_account_unless_it_captures() {
        let (account, other) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let (window, call) = (popup_label(account, 1), popup_label(account, 2));
        let mut mutes = WindowMutes::default();
        assert!(!mutes.window_muted(account, &window));
        mutes.set_account_muted(account, true);
        mutes.set_window_capturing(&call, true);
        assert!(mutes.window_muted(account, &window));
        assert!(!mutes.window_muted(account, &call));
        assert!(!mutes.window_muted(other, &popup_label(other, 1)));
        mutes.set_window_capturing(&call, false);
        assert!(mutes.window_muted(account, &call));
        mutes.set_account_muted(account, false);
        assert!(!mutes.window_muted(account, &window));
    }

    #[test]
    fn forgetting_an_account_forgets_its_windows() {
        let (account, other) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut mutes = WindowMutes::default();
        mutes.set_account_muted(account, true);
        mutes.set_window_capturing(&popup_label(account, 1), true);
        mutes.set_window_capturing(&popup_label(other, 1), true);
        mutes.forget_account(account);
        assert!(!mutes.window_muted(account, &popup_label(account, 2)));
        assert_eq!(
            mutes.capturing_windows,
            HashSet::from([popup_label(other, 1)])
        );
    }
}
