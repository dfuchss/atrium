//! When and how a page's JavaScript dialog (`alert`, `confirm`, `prompt`) is shown: its title
//! and message, the rate limit and the suppression a page's dialogs get. The host converts from
//! and to its native dialog types.
//!
//! - The title names the account (its sidebar label) and the host of the asking frame.
//! - From a page's second dialog on, the user is offered to prevent further dialogs; accepted,
//!   the account's further dialogs get the default answer until its view loads a page again.
//! - At most [`MAX_DIALOGS_PER_MINUTE`] per account are shown; beyond that the default answer
//!   is given.

use std::collections::HashMap;

use protocol::Snapshot;
use uuid::Uuid;

use crate::core::{MS_PER_MINUTE, account_label};
use crate::throttle::RateLimiter;

const MAX_DIALOGS_PER_MINUTE: usize = 10;
/// Longer messages are cut: a panel is no place for a page's novel.
const MAX_MESSAGE_CHARS: usize = 1_000;
/// Dialog number (per page) from which the suppression checkbox is offered.
const OFFER_SUPPRESSION_FROM: u32 = 2;

/// `host`, `host:port` for a non-default port (`port` 0), or None for pages without a host.
pub fn dialog_origin_label(host: &str, port: u16) -> Option<String> {
    if host.is_empty() {
        None
    } else if port == 0 {
        Some(host.to_string())
    } else {
        Some(format!("{host}:{port}"))
    }
}

/// `"<account>: <origin> says"`; without an origin the page is "a page".
fn dialog_title(account_name: Option<&str>, origin: Option<&str>) -> String {
    let source = origin.unwrap_or("a page");
    match account_name {
        Some(name) => format!("{name}: {source} says"),
        None => format!("{source} says"),
    }
}

/// The first [`MAX_MESSAGE_CHARS`] characters, with an ellipsis when cut.
fn clip_message(message: &str) -> String {
    let mut chars = message.char_indices().skip(MAX_MESSAGE_CHARS);
    match chars.next() {
        Some((end, _)) => format!("{}\u{2026}", &message[..end]),
        None => message.to_string(),
    }
}

/// What a shown dialog says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogText {
    pub title: String,
    pub message: String,
    /// Offer "Prevent this page from creating additional dialogs".
    pub offer_suppression: bool,
}

/// Why a dialog gets the default answer instead of being shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withheld {
    /// The user asked to prevent further dialogs of the page.
    Suppressed,
    RateLimited,
}

#[derive(Default)]
struct AccountDialogs {
    shown_since_load: u32,
    suppressed: bool,
}

/// Decides per account whether and how a dialog is shown.
pub struct DialogPolicy {
    names: HashMap<Uuid, String>,
    accounts: HashMap<Uuid, AccountDialogs>,
    limiter: RateLimiter<Uuid>,
}

impl Default for DialogPolicy {
    fn default() -> DialogPolicy {
        DialogPolicy {
            names: HashMap::new(),
            accounts: HashMap::new(),
            limiter: RateLimiter::new(MAX_DIALOGS_PER_MINUTE, MS_PER_MINUTE),
        }
    }
}

impl DialogPolicy {
    /// Takes the account names for the titles (sidebar label, else service name) from
    /// `snapshot`.
    pub fn set_names(&mut self, snapshot: &Snapshot) {
        self.names = snapshot
            .accounts
            .iter()
            .map(|account| {
                (
                    account.id,
                    account_label(&account.settings.label, &account.service_name),
                )
            })
            .collect();
    }

    /// The view of `account` loaded a page: earlier dialogs and a suppression no longer count.
    pub fn page_loaded(&mut self, account: Uuid) {
        self.accounts.remove(&account);
    }

    /// Text of a dialog of `account` (None: a webview without account, counted as one
    /// anonymous account) from `origin` (see [`dialog_origin_label`]), or why it is withheld.
    pub fn present(
        &mut self,
        account: Option<Uuid>,
        origin: Option<&str>,
        message: &str,
        now_ms: u64,
    ) -> Result<DialogText, Withheld> {
        let key = account.unwrap_or_default();
        let state = self.accounts.entry(key).or_default();
        if state.suppressed {
            return Err(Withheld::Suppressed);
        }
        if !self.limiter.allow(key, now_ms) {
            return Err(Withheld::RateLimited);
        }
        state.shown_since_load += 1;
        let name = account
            .and_then(|id| self.names.get(&id))
            .map(String::as_str);
        Ok(DialogText {
            title: dialog_title(name, origin),
            message: clip_message(message),
            offer_suppression: state.shown_since_load >= OFFER_SUPPRESSION_FROM,
        })
    }

    /// The user answered a dialog of `account`, maybe asking for no further ones.
    pub fn answered(&mut self, account: Option<Uuid>, suppress_further: bool) {
        if suppress_further {
            self.accounts
                .entry(account.unwrap_or_default())
                .or_default()
                .suppressed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use protocol::AccountView;

    use super::*;
    use crate::Account;

    fn snapshot_with(id: Uuid, label: &str, service_name: &str) -> Snapshot {
        Snapshot {
            accounts: vec![AccountView {
                id,
                service: "service".into(),
                service_name: service_name.into(),
                color: String::new(),
                initials: String::new(),
                unread: 0,
                indirect: false,
                instance_label: None,
                icon: None,
                icon_glyph: false,
                hibernated: false,
                settings: Account::new(id, "service", label, None).settings,
            }],
            ..Snapshot::default()
        }
    }

    #[test]
    fn titles_name_account_and_origin() {
        assert_eq!(
            dialog_origin_label("app.slack.com", 0).as_deref(),
            Some("app.slack.com")
        );
        assert_eq!(
            dialog_origin_label("localhost", 8080).as_deref(),
            Some("localhost:8080")
        );
        assert_eq!(dialog_origin_label("", 0), None);
        assert_eq!(
            dialog_title(Some("Work"), Some("app.slack.com")),
            "Work: app.slack.com says"
        );
        assert_eq!(
            dialog_title(None, Some("app.slack.com")),
            "app.slack.com says"
        );
        assert_eq!(dialog_title(Some("Work"), None), "Work: a page says");
    }

    #[test]
    fn long_messages_are_clipped_at_a_char_boundary() {
        assert_eq!(clip_message("short"), "short");
        let long = "ä".repeat(MAX_MESSAGE_CHARS + 5);
        let clipped = clip_message(&long);
        assert_eq!(clipped.chars().count(), MAX_MESSAGE_CHARS + 1);
        assert!(clipped.ends_with('\u{2026}'));
        assert_eq!(
            clip_message(&"x".repeat(MAX_MESSAGE_CHARS)).len(),
            MAX_MESSAGE_CHARS
        );
    }

    #[test]
    fn suppression_is_offered_from_the_second_dialog_and_ends_with_a_page_load() {
        let id = Uuid::from_u128(1);
        let origin = Some("app.slack.com");
        let mut policy = DialogPolicy::default();
        policy.set_names(&snapshot_with(id, "Work", "Slack"));
        assert_eq!(
            policy.present(Some(id), origin, "Leave?", 0),
            Ok(DialogText {
                title: "Work: app.slack.com says".into(),
                message: "Leave?".into(),
                offer_suppression: false,
            })
        );
        let second = policy.present(Some(id), origin, "Sure?", 1);
        assert!(second.is_ok_and(|text| text.offer_suppression));
        policy.answered(Some(id), true);
        assert_eq!(
            policy.present(Some(id), origin, "Again?", 2),
            Err(Withheld::Suppressed)
        );
        policy.page_loaded(id);
        let after_load = policy.present(Some(id), origin, "Again?", 3);
        assert!(after_load.is_ok_and(|text| !text.offer_suppression));
    }

    #[test]
    fn dialogs_are_rate_limited_per_account() {
        let (first, second) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut policy = DialogPolicy::default();
        for _ in 0..MAX_DIALOGS_PER_MINUTE {
            assert!(
                policy
                    .present(Some(first), Some("a.example"), "hi", 0)
                    .is_ok()
            );
        }
        assert_eq!(
            policy.present(Some(first), Some("a.example"), "hi", 100),
            Err(Withheld::RateLimited)
        );
        assert!(
            policy
                .present(Some(second), Some("b.example"), "hi", 100)
                .is_ok()
        );
        assert!(
            policy
                .present(Some(first), Some("a.example"), "hi", MS_PER_MINUTE)
                .is_ok()
        );
    }

    #[test]
    fn names_fall_back_to_the_service_and_unknown_webviews_get_none() {
        let id = Uuid::from_u128(7);
        let mut policy = DialogPolicy::default();
        policy.set_names(&snapshot_with(id, "  ", "Element"));
        let named = policy.present(Some(id), None, "hi", 0);
        assert!(named.is_ok_and(|text| text.title == "Element: a page says"));
        let anonymous = policy.present(None, Some("x.example"), "hi", 0);
        assert!(anonymous.is_ok_and(|text| text.title == "x.example says"));
    }
}
