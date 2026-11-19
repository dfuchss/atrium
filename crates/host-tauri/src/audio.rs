//! The audio mute of an account's in-app windows: they follow its view, which the core mutes
//! ([`domain::Effect::SetAudioMuted`]); their capture goes to the core like the view's
//! ([`domain::Input::CaptureChanged`]).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use fs_util::lock_ignoring_poison;
use tauri::{Runtime, WebviewWindow, WindowEvent};
use uuid::Uuid;

use crate::{Feed, Input, Logger, native};

/// The accounts whose last `SetAudioMuted` muted them. The mute is applied under this lock, so
/// a newer value is never overtaken by an older one.
pub(crate) type MutedAccounts = Arc<Mutex<HashSet<Uuid>>>;

/// Gives `account`'s new in-app `window` the account's mute and reports its capture to the
/// core until it closes.
pub(crate) fn watch_window<R: Runtime>(
    window: &WebviewWindow<R>,
    account: Uuid,
    muted: &MutedAccounts,
    feed: &Feed,
    logger: &Logger,
) {
    let muted = lock_ignoring_poison(muted);
    if muted.contains(&account) {
        native::set_audio_muted(window.as_ref(), true, logger);
    }
    drop(muted);
    let label = window.label().to_string();
    let (capture_feed, capture_label) = (feed.clone(), label.clone());
    native::observe_capture(
        window.as_ref(),
        Box::new(move |active| {
            capture_feed(Input::CaptureChanged {
                account,
                window: Some(capture_label.clone()),
                active,
            });
        }),
        logger,
    );
    let feed = feed.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            feed(Input::CaptureChanged {
                account,
                window: Some(label.clone()),
                active: false,
            });
        }
    });
}
