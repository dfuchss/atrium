//! When and how a dialog is shown: its title and message, the rate limit and the suppression
//! a page's dialogs get.

use std::collections::HashMap;

use domain::RateLimiter;
use platform_ffi::{DialogRequest, DialogText, PageOrigin};
use protocol::Snapshot;
use uuid::Uuid;

pub(crate) const MAX_DIALOGS_PER_MINUTE: usize = 10;
const MS_PER_MINUTE: u64 = 60_000;
/// Longer messages are cut: a panel is no place for a page's novel.
const MAX_MESSAGE_CHARS: usize = 1_000;
/// Dialog number (per page) from which the suppression checkbox is offered.
const OFFER_SUPPRESSION_FROM: u32 = 2;

/// `host`, `host:port` for a non-default port, or None for pages without a host.
pub(crate) fn origin_label(origin: &PageOrigin) -> Option<String> {
    if origin.host.is_empty() {
        None
    } else if origin.port == 0 {
        Some(origin.host.clone())
    } else {
        Some(format!("{}:{}", origin.host, origin.port))
    }
}

/// `"<account>: <host> says"`; without a host the page is "a page".
pub(crate) fn dialog_title(account_name: Option<&str>, origin: Option<&str>) -> String {
    let source = origin.unwrap_or("a page");
    match account_name {
        Some(name) => format!("{name}: {source} says"),
        None => format!("{source} says"),
    }
}

/// The first [`MAX_MESSAGE_CHARS`] characters, with an ellipsis when cut.
pub(crate) fn clip_message(message: &str) -> String {
    let mut chars = message.char_indices().skip(MAX_MESSAGE_CHARS);
    match chars.next() {
        Some((end, _)) => format!("{}\u{2026}", &message[..end]),
        None => message.to_string(),
    }
}

/// Sidebar label of an account, else its service name.
pub(crate) fn display_name(label: &str, service_name: &str) -> String {
    let label = label.trim();
    if label.is_empty() {
        service_name.to_string()
    } else {
        label.to_string()
    }
}

/// The accounts of `snapshot` with their names for the dialog titles.
pub(super) fn account_names(snapshot: &Snapshot) -> impl Iterator<Item = (Uuid, String)> + '_ {
    snapshot.accounts.iter().map(|account| {
        (
            account.id,
            display_name(&account.settings.label, &account.service_name),
        )
    })
}

/// Why a dialog gets the default answer instead of being shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Withheld {
    /// The user ticked "Prevent this page from creating additional dialogs".
    Suppressed,
    RateLimited,
}

#[derive(Default)]
struct AccountDialogs {
    shown_since_load: u32,
    suppressed: bool,
}

/// Decides per account whether and how a dialog is shown.
pub(crate) struct DialogPolicy {
    names: HashMap<Uuid, String>,
    accounts: HashMap<Uuid, AccountDialogs>,
    limiter: RateLimiter<Uuid>,
}

impl DialogPolicy {
    pub(crate) fn new() -> DialogPolicy {
        DialogPolicy {
            names: HashMap::new(),
            accounts: HashMap::new(),
            limiter: RateLimiter::new(MAX_DIALOGS_PER_MINUTE, MS_PER_MINUTE),
        }
    }

    /// The account names for the titles (see [`account_names`]).
    pub(crate) fn set_names(&mut self, names: impl IntoIterator<Item = (Uuid, String)>) {
        self.names = names.into_iter().collect();
    }

    /// The view of `account` loaded a page: earlier dialogs and a suppression no longer count.
    pub(crate) fn page_loaded(&mut self, account: Uuid) {
        self.accounts.remove(&account);
    }

    /// Text of the dialog `request` of `account` (None: a webview without label, counted as
    /// one anonymous account), or why it is withheld.
    pub(crate) fn present(
        &mut self,
        account: Option<Uuid>,
        request: &DialogRequest,
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
            title: dialog_title(name, origin_label(&request.origin).as_deref()),
            message: clip_message(&request.message),
            offer_suppression: state.shown_since_load >= OFFER_SUPPRESSION_FROM,
        })
    }

    /// The user answered a dialog of `account`, maybe asking for no further ones.
    pub(crate) fn answered(&mut self, account: Option<Uuid>, suppress_further: bool) {
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
    use platform_ffi::DialogKind;

    use super::*;

    fn request(host: &str, port: u16, message: &str) -> DialogRequest {
        DialogRequest {
            kind: DialogKind::Confirm,
            message: message.into(),
            origin: PageOrigin {
                host: host.into(),
                port,
            },
            webview_tag: None,
        }
    }

    #[test]
    fn titles_name_account_and_origin() {
        let origin = PageOrigin {
            host: "app.slack.com".into(),
            port: 0,
        };
        assert_eq!(origin_label(&origin).as_deref(), Some("app.slack.com"));
        let with_port = PageOrigin {
            host: "localhost".into(),
            port: 8080,
        };
        assert_eq!(origin_label(&with_port).as_deref(), Some("localhost:8080"));
        assert_eq!(
            origin_label(&PageOrigin {
                host: String::new(),
                port: 0
            }),
            None
        );
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
        let mut policy = DialogPolicy::new();
        policy.set_names([(id, "Work".to_string())]);
        let first = policy.present(Some(id), &request("app.slack.com", 0, "Leave?"), 0);
        assert_eq!(
            first,
            Ok(DialogText {
                title: "Work: app.slack.com says".into(),
                message: "Leave?".into(),
                offer_suppression: false,
            })
        );
        let second = policy.present(Some(id), &request("app.slack.com", 0, "Sure?"), 1);
        assert!(second.is_ok_and(|text| text.offer_suppression));
        policy.answered(Some(id), true);
        assert_eq!(
            policy.present(Some(id), &request("app.slack.com", 0, "Again?"), 2),
            Err(Withheld::Suppressed)
        );
        policy.page_loaded(id);
        let after_load = policy.present(Some(id), &request("app.slack.com", 0, "Again?"), 3);
        assert!(after_load.is_ok_and(|text| !text.offer_suppression));
    }

    #[test]
    fn dialogs_are_rate_limited_per_account() {
        let (first, second) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut policy = DialogPolicy::new();
        for _ in 0..MAX_DIALOGS_PER_MINUTE {
            assert!(
                policy
                    .present(Some(first), &request("a.example", 0, "hi"), 0)
                    .is_ok()
            );
        }
        assert_eq!(
            policy.present(Some(first), &request("a.example", 0, "hi"), 100),
            Err(Withheld::RateLimited)
        );
        assert!(
            policy
                .present(Some(second), &request("b.example", 0, "hi"), 100)
                .is_ok()
        );
        assert!(
            policy
                .present(Some(first), &request("a.example", 0, "hi"), MS_PER_MINUTE)
                .is_ok()
        );
    }

    #[test]
    fn names_fall_back_to_the_service_and_unknown_webviews_get_none() {
        assert_eq!(display_name("Work", "Slack"), "Work");
        assert_eq!(display_name("  ", "Element"), "Element");
        let id = Uuid::from_u128(7);
        let mut policy = DialogPolicy::new();
        policy.set_names([(id, "Element".to_string())]);
        let named = policy.present(Some(id), &request("", 0, "hi"), 0);
        assert!(named.is_ok_and(|text| text.title == "Element: a page says"));
        let anonymous = policy.present(None, &request("x.example", 0, "hi"), 0);
        assert!(anonymous.is_ok_and(|text| text.title == "x.example says"));
    }
}
