//! Native notifications and the routing of their clicks back to the page that sent them.

use std::collections::HashMap;
use std::sync::PoisonError;

use domain::NotificationReq;
use domain::ports::{Notifier, PortResult};
use tauri::Runtime;
use tauri_plugin_notification::NotificationExt;
use uuid::Uuid;

use crate::{Input, TauriHost, port_err};

/// Sound name that makes `NSUserNotification` play the user's default notification sound
/// (mac-notification-sys maps exactly this string to `NSUserNotificationDefaultSoundName`;
/// other strings are looked up as named system sounds, unknown names play nothing).
const DEFAULT_SOUND: &str = "NSUserNotificationDefaultSoundName";

/// Native notifications remembered for click routing; older ones are forgotten.
const MAX_TRACKED_NOTIFICATIONS: u16 = 512;

/// Which page notification each native notification stands for.
#[derive(Default)]
pub(crate) struct NotificationTargets {
    last_id: i32,
    /// Native notification id -> (account, shim id if the page sent it).
    targets: HashMap<i32, (Uuid, Option<u64>)>,
}

impl NotificationTargets {
    /// Assigns the next native id to the notification `shim_id` of `account`.
    fn track(&mut self, account: Uuid, shim_id: Option<u64>) -> i32 {
        self.last_id = self.last_id.checked_add(1).unwrap_or(1);
        let native_id = self.last_id;
        self.targets.insert(native_id, (account, shim_id));
        if self.targets.len() > usize::from(MAX_TRACKED_NOTIFICATIONS) {
            let oldest_kept = native_id.saturating_sub(i32::from(MAX_TRACKED_NOTIFICATIONS));
            self.targets.retain(|&tracked, _| tracked > oldest_kept);
        }
        native_id
    }

    fn target(&self, native_id: i32) -> Option<(Uuid, Option<u64>)> {
        self.targets.get(&native_id).copied()
    }
}

impl<R: Runtime> TauriHost<R> {
    /// Routes clicks on native notifications back as [`Input::NotificationClicked`].
    /// Called by [`TauriHost::new`]; notifications shown before registration report nothing.
    pub(crate) fn register_notification_clicks(&self) {
        let (feed, notifications) = (self.feed.clone(), self.notifications.clone());
        let result = self.app.notification().on_action(move |action| {
            let Some(native_id) = action
                .notification()
                .map(tauri_plugin_notification::ActiveNotification::id)
            else {
                return;
            };
            let target = notifications
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .target(native_id);
            if let Some((account, shim_id)) = target {
                feed(Input::NotificationClicked { account, shim_id });
            }
        });
        if let Err(error) = result {
            self.logger.log(&format!(
                "cannot register notification click handler: {error}"
            ));
        }
    }
}

impl<R: Runtime> Notifier for TauriHost<R> {
    fn notify(&self, request: &NotificationReq) -> PortResult {
        let native_id = self
            .notifications
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .track(request.account, request.shim_id);
        let mut builder = self
            .app
            .notification()
            .builder()
            .id(native_id)
            .title(&request.title)
            .body(&request.body);
        if request.sound {
            builder = builder.sound(DEFAULT_SOUND);
        }
        builder.show().map_err(port_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_the_latest_notifications_only() {
        let mut targets = NotificationTargets::default();
        let account = Uuid::from_u128(1);
        let first = targets.track(account, Some(10));
        assert_eq!(targets.target(first), Some((account, Some(10))));
        for shim_id in 0..u64::from(MAX_TRACKED_NOTIFICATIONS) {
            targets.track(account, Some(shim_id));
        }
        assert_eq!(targets.target(first), None);
        assert_eq!(
            targets.targets.len(),
            usize::from(MAX_TRACKED_NOTIFICATIONS)
        );
    }

    #[test]
    fn native_ids_wrap_to_one() {
        let mut targets = NotificationTargets {
            last_id: i32::MAX,
            ..NotificationTargets::default()
        };
        assert_eq!(targets.track(Uuid::from_u128(1), None), 1);
    }
}
