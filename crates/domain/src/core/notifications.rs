//! Native notifications: the web notifications a page's poll drained, and finished downloads.

use uuid::Uuid;

use super::{Core, Effect, NotificationReq};

/// Body of notifications when previews are off.
pub(super) const HIDDEN_PREVIEW_BODY: &str = "New message";

/// Notifications shown from one poll; the rest of a burst is dropped.
const NOTIFICATIONS_PER_POLL: usize = 5;
/// Notifications shown per account and minute.
pub(super) const NOTIFICATIONS_PER_MINUTE: usize = 20;
/// Longest notification title and body taken from a page, in characters.
const NOTIFICATION_TITLE_MAX_CHARS: usize = 200;
const NOTIFICATION_BODY_MAX_CHARS: usize = 1000;

/// One web notification drained from the page's shim queue.
#[derive(serde::Deserialize)]
pub(super) struct PolledNotification {
    id: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
}

/// The first `max_chars` characters of `text`.
fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// Notification title prefix: the service name, plus the label if it differs.
pub(super) fn notification_prefix(service_name: &str, label: &str) -> String {
    if label == service_name {
        service_name.to_string()
    } else {
        format!("{service_name} ({label})")
    }
}

impl Core {
    /// Native notifications for the drained web notifications, unless Do Not Disturb is on.
    /// A page gets at most [`NOTIFICATIONS_PER_POLL`] per poll and [`NOTIFICATIONS_PER_MINUTE`]
    /// per minute, with title and body cut to a readable length; the rest is dropped.
    pub(super) fn notifications(
        &mut self,
        account_id: Uuid,
        prefix: &str,
        notifications: Vec<PolledNotification>,
    ) -> Vec<Effect> {
        let settings = self.settings();
        if settings.dnd {
            return vec![];
        }
        let (preview, sound) = (settings.notification_preview, settings.notification_sound);
        let now_ms = self.last_tick_ms.unwrap_or(0);
        let received = notifications.len();
        let mut effects: Vec<Effect> = notifications
            .into_iter()
            .take(NOTIFICATIONS_PER_POLL)
            .filter(|_| self.notification_limit.allow(account_id, now_ms))
            .map(|notification| {
                Effect::Notify(NotificationReq {
                    account: account_id,
                    shim_id: Some(notification.id),
                    title: format!(
                        "{prefix}: {}",
                        truncate_chars(&notification.title, NOTIFICATION_TITLE_MAX_CHARS)
                    ),
                    body: if preview {
                        truncate_chars(
                            notification.body.as_deref().unwrap_or_default(),
                            NOTIFICATION_BODY_MAX_CHARS,
                        )
                    } else {
                        HIDDEN_PREVIEW_BODY.to_string()
                    },
                    sound,
                })
            })
            .collect();
        if effects.len() < received {
            effects.push(Effect::Log(format!(
                "[{account_id}] dropped {} of {received} notifications (rate limit)",
                received - effects.len()
            )));
        }
        effects
    }

    /// Tells the user that a download of the account landed in the Downloads folder (the
    /// page cannot show it); silent under Do Not Disturb and for a muted account.
    pub(super) fn download_finished(&self, account_id: Uuid, file_name: &str) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        let settings = self.settings();
        if settings.dnd || account.settings.muted {
            return vec![];
        }
        let service_name = self
            .registry
            .get(&account.service)
            .map_or(account.service.as_str(), |service| service.name.as_str());
        vec![Effect::Notify(NotificationReq {
            account: account_id,
            shim_id: None,
            title: format!("Downloaded {file_name}"),
            body: notification_prefix(service_name, &account.settings.label),
            sound: settings.notification_sound,
        })]
    }
}
