//! Unread counts (from the page title or a poll), web notifications and the dock badge.

use std::collections::{HashMap, HashSet};

use protocol::DockBadgeMode;
use regex::Regex;
use serde_json::Value;
use uuid::Uuid;

use super::{Core, DockBadge, Effect, NotificationReq};
use crate::catalog::ServiceDef;

/// Body of notifications when previews are off.
pub const HIDDEN_PREVIEW_BODY: &str = "New message";

/// Unread state of one account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Unread {
    pub(super) direct: u32,
    pub(super) indirect: bool,
}

/// Compiled title patterns of one service.
#[derive(Default)]
pub(super) struct TitleRules {
    direct: Option<Regex>,
    indirect: Option<Regex>,
}

impl TitleRules {
    /// Invalid patterns count as missing (the catalog validates them on load).
    pub(super) fn compile(service: &ServiceDef) -> TitleRules {
        let compile = |pattern: &Option<String>| {
            pattern
                .as_deref()
                .and_then(|pattern| Regex::new(pattern).ok())
        };
        TitleRules {
            direct: compile(&service.unread.title_regex),
            indirect: compile(&service.unread.title_indirect_regex),
        }
    }

    /// Group 1 of the direct pattern is the count; a match of the indirect one sets the dot.
    fn parse(&self, title: &str) -> Unread {
        let direct = self
            .direct
            .as_ref()
            .and_then(|pattern| pattern.captures(title))
            .and_then(|captures| captures.get(1))
            .and_then(|count| count.as_str().parse::<u32>().ok())
            .unwrap_or(0);
        let indirect = self
            .indirect
            .as_ref()
            .is_some_and(|pattern| pattern.is_match(title));
        Unread { direct, indirect }
    }
}

/// Unread state of all accounts and where it came from.
#[derive(Default)]
pub(super) struct UnreadTracker {
    /// Effective unread per account.
    current: HashMap<Uuid, Unread>,
    /// Last unread derived from the window title (fallback when a poll yields no value).
    from_title: HashMap<Uuid, Unread>,
    /// Accounts whose last poll yielded a value (it then wins over the title).
    polled: HashSet<Uuid>,
}

impl UnreadTracker {
    pub(super) fn of(&self, account_id: Uuid) -> Unread {
        self.current.get(&account_id).copied().unwrap_or_default()
    }

    pub(super) fn forget(&mut self, account_id: Uuid) {
        self.current.remove(&account_id);
        self.from_title.remove(&account_id);
        self.polled.remove(&account_id);
    }
}

/// One web notification drained from the page's shim queue.
#[derive(serde::Deserialize)]
struct PolledNotification {
    id: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
}

/// Decoded result of the poll script.
struct PollResult {
    notifications: Vec<PolledNotification>,
    /// The unread expression's value; None if it yielded nothing usable.
    unread: Option<Unread>,
}

impl PollResult {
    /// `raw` is the JSON encoding of the script's string result (tolerated without the outer
    /// string encoding).
    fn decode(raw: &str) -> Result<PollResult, String> {
        let value = serde_json::from_str::<Value>(raw)
            .and_then(|value| match value {
                Value::String(inner) => serde_json::from_str(&inner),
                other => Ok(other),
            })
            .map_err(|error| format!("bad JSON: {error}"))?;
        let notifications = match value.get("n") {
            None | Some(Value::Null) => vec![],
            Some(queue) => serde_json::from_value(queue.clone())
                .map_err(|error| format!("bad notifications: {error}"))?,
        };
        Ok(PollResult {
            notifications,
            unread: polled_unread(value.get("u")),
        })
    }
}

/// A number is a direct count; an object has `direct` and `indirect`; anything else is no value.
fn polled_unread(value: Option<&Value>) -> Option<Unread> {
    match value? {
        Value::Number(number) => Some(Unread {
            direct: number.as_u64().map_or_else(
                || saturating_count(number.as_f64().unwrap_or(0.0)),
                |count| u32::try_from(count).unwrap_or(u32::MAX),
            ),
            indirect: false,
        }),
        object @ Value::Object(_) => Some(Unread {
            direct: saturating_count(object.get("direct").and_then(Value::as_f64).unwrap_or(0.0)),
            indirect: object
                .get("indirect")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        _ => None,
    }
}

/// A count reported by a page: negative and NaN become 0, fractions are cut off and huge
/// values saturate.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "float to int `as` casts saturate, which is what we want"
)]
fn saturating_count(value: f64) -> u32 {
    value.max(0.0) as u32
}

/// Notification title prefix: the service name, plus the label if it differs.
fn notification_prefix(service_name: &str, label: &str) -> String {
    if label == service_name {
        service_name.to_string()
    } else {
        format!("{service_name} ({label})")
    }
}

impl Core {
    pub(super) fn title_changed(&mut self, account_id: Uuid, title: &str) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        let service_id = account.service.clone();
        let Some(service) = self.catalog.get(&service_id) else {
            return vec![];
        };
        let has_poll = service.unread.poll.is_some();
        let unread = self
            .title_rules
            .get(&service_id)
            .map_or_default(|rules| rules.parse(title));
        self.unread.from_title.insert(account_id, unread);
        if has_poll && self.unread.polled.contains(&account_id) {
            return vec![];
        }
        let mut effects = vec![];
        if self.unread.of(account_id) != unread {
            // Lets the user check the title patterns against what the service really shows.
            effects.push(Effect::Log(format!(
                "[{account_id}] {service_id} title {title:?} -> unread {}, indirect {}",
                unread.direct, unread.indirect
            )));
        }
        effects.extend(self.update_unread(account_id, unread));
        effects
    }

    pub(super) fn poll_result(&mut self, account_id: Uuid, raw: &str) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        let (muted, service_id) = (account.muted, account.service.clone());
        let Some(service) = self.catalog.get(&service_id) else {
            return vec![Effect::Log(format!(
                "poll result: unknown service {service_id}"
            ))];
        };
        let has_poll = service.unread.poll.is_some();
        let prefix = notification_prefix(&service.name, &account.label);
        let poll = match PollResult::decode(raw) {
            Ok(poll) => poll,
            Err(error) => {
                return vec![Effect::Log(format!(
                    "poll result for {account_id}: {error}"
                ))];
            }
        };
        let mut effects = if muted {
            vec![]
        } else {
            self.notifications(account_id, &prefix, poll.notifications)
        };
        if has_poll {
            effects.extend(self.apply_polled_unread(account_id, &service_id, poll.unread));
        }
        effects
    }

    /// Native notifications for the drained web notifications, unless Do Not Disturb is on.
    fn notifications(
        &self,
        account_id: Uuid,
        prefix: &str,
        notifications: Vec<PolledNotification>,
    ) -> Vec<Effect> {
        let settings = self.settings();
        if settings.dnd {
            return vec![];
        }
        notifications
            .into_iter()
            .map(|notification| {
                Effect::Notify(NotificationReq {
                    account: account_id,
                    shim_id: notification.id,
                    title: format!("{prefix}: {}", notification.title),
                    body: if settings.notification_preview {
                        notification.body.unwrap_or_default()
                    } else {
                        HIDDEN_PREVIEW_BODY.to_string()
                    },
                    sound: settings.notification_sound,
                })
            })
            .collect()
    }

    /// A polled value wins over the title; without one the title-derived unread applies again.
    fn apply_polled_unread(
        &mut self,
        account_id: Uuid,
        service_id: &str,
        polled: Option<Unread>,
    ) -> Vec<Effect> {
        let mut effects = vec![];
        if let Some(unread) = polled {
            self.unread.polled.insert(account_id);
            if self.unread.of(account_id) != unread {
                effects.push(Effect::Log(format!(
                    "[{account_id}] {service_id} poll -> unread {}, indirect {}",
                    unread.direct, unread.indirect
                )));
            }
            effects.extend(self.update_unread(account_id, unread));
        } else {
            if self.unread.polled.remove(&account_id) {
                effects.push(Effect::Log(format!(
                    "[{account_id}] {service_id} poll -> no value, using title"
                )));
            }
            if let Some(from_title) = self.unread.from_title.get(&account_id).copied() {
                effects.extend(self.update_unread(account_id, from_title));
            }
        }
        effects
    }

    /// Stores new unread state; returns the follow-up effects if it changed.
    fn update_unread(&mut self, account_id: Uuid, unread: Unread) -> Vec<Effect> {
        if self.unread.of(account_id) == unread {
            return vec![];
        }
        self.unread.current.insert(account_id, unread);
        let mut effects: Vec<Effect> = self.dock_badge().into_iter().collect();
        effects.push(self.emit());
        effects
    }

    /// Sum of the unread of the enabled, unmuted accounts counted in the dock badge.
    pub(super) fn badge(&self) -> DockBadge {
        let mut count = 0u32;
        let mut dot = false;
        for account in self
            .persisted
            .accounts
            .iter()
            .filter(|account| account.in_dock_badge && !account.muted && account.enabled)
        {
            let unread = self.unread.of(account.id);
            count = count.saturating_add(unread.direct);
            dot |= unread.indirect;
        }
        match self.settings().dock_badge {
            DockBadgeMode::Off => DockBadge::None,
            _ if count > 0 => DockBadge::Count(count),
            DockBadgeMode::All if dot => DockBadge::Dot,
            _ => DockBadge::None,
        }
    }

    /// Recomputes the dock badge; returns an effect only if it changed.
    pub(super) fn dock_badge(&mut self) -> Option<Effect> {
        let badge = self.badge();
        if self.last_badge == Some(badge) {
            return None;
        }
        self.last_badge = Some(badge);
        Some(Effect::SetDockBadge(badge))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polled_unread_accepts_numbers_and_objects() {
        let unread = |json: &str| polled_unread(Some(&serde_json::from_str(json).unwrap()));
        let direct = |direct| {
            Some(Unread {
                direct,
                indirect: false,
            })
        };
        assert_eq!(unread("3"), direct(3));
        assert_eq!(unread("2.9"), direct(2));
        assert_eq!(unread("-4"), direct(0));
        assert_eq!(unread("1e12"), direct(u32::MAX));
        assert_eq!(unread("8589934592"), direct(u32::MAX));
        assert_eq!(
            unread(r#"{"direct": 5, "indirect": true}"#),
            Some(Unread {
                direct: 5,
                indirect: true
            })
        );
        assert_eq!(unread("{}"), direct(0));
        assert_eq!(unread("null"), None);
        assert_eq!(unread(r#""7""#), None);
        assert_eq!(polled_unread(None), None);
    }
}
