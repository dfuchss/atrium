//! Native notifications and the routing of their clicks back to the page that sent them. On
//! Windows a guarded instance shows toasts of its own, whose clicks come as links (see
//! `native::windows::notification_links`); elsewhere the notification plugin shows them and
//! reports their clicks.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use domain::NotificationReq;
use domain::ports::{Notifier, PortResult};
use fs_util::lock_ignoring_poison;
use tauri::{Manager as _, Runtime};
use tauri_plugin_notification::NotificationExt;
use uuid::Uuid;

use crate::{Feed, Input, TauriHost, native, port_err};

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

/// Feeds clicks on tracked native notifications as [`Input::NotificationClicked`]; in the app
/// state for the notification links of Windows.
#[derive(Clone)]
pub(crate) struct NotificationClicks {
    feed: Feed,
    targets: Arc<Mutex<NotificationTargets>>,
}

impl NotificationClicks {
    /// Feeds the click on the native notification `native_id`; false if it is not tracked (never
    /// shown or forgotten).
    pub(crate) fn clicked(&self, native_id: i32) -> bool {
        let target = lock_ignoring_poison(&self.targets).target(native_id);
        target
            .map(|(account, shim_id)| (self.feed)(Input::NotificationClicked { account, shim_id }))
            .is_some()
    }
}

impl<R: Runtime> TauriHost<R> {
    /// Routes clicks on native notifications back as [`Input::NotificationClicked`].
    /// Called by [`TauriHost::new`]; notifications shown before registration report nothing.
    pub(crate) fn register_notification_clicks(&self) {
        let clicks = NotificationClicks {
            feed: self.feed.clone(),
            targets: self.notifications.clone(),
        };
        self.app.manage(clicks.clone());
        let result = self.app.notification().on_action(move |action| {
            if let Some(notification) = action.notification() {
                clicks.clicked(notification.id());
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
        let native_id =
            lock_ignoring_poison(&self.notifications).track(request.account, request.shim_id);
        #[cfg(windows)]
        if let Some(links) = self.app.try_state::<native::NotificationLinks>() {
            return native::show_linked_notification(&links, native_id, request);
        }
        let mut builder = self
            .app
            .notification()
            .builder()
            .id(native_id)
            .title(&request.title)
            .body(&request.body);
        if request.sound {
            builder = builder.sound(native::NOTIFICATION_SOUND);
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
