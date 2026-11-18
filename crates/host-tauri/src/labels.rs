//! The labels of the webviews and windows of an account, which name the account they belong
//! to: `svc-<id>` for its service view, `popup-<id>-<n>` for the in-app windows it opens (`id`
//! is the account's simple UUID). Every other webview (the sidebar `ui`, dialog windows) belongs
//! to no account.

use uuid::Uuid;

/// Start of the labels of service webviews.
const VIEW_LABEL_PREFIX: &str = "svc-";
/// Start of the labels of in-app windows.
const POPUP_LABEL_PREFIX: &str = "popup-";

/// Label of the service webview of `account`.
pub(crate) fn view_label(account: Uuid) -> String {
    format!("{VIEW_LABEL_PREFIX}{}", account.simple())
}

/// The account whose service webview has `label`; None for any other webview (for the crash
/// hooks).
pub(crate) fn view_account(label: &str) -> Option<Uuid> {
    Uuid::try_parse(label.strip_prefix(VIEW_LABEL_PREFIX)?).ok()
}

/// Label of an in-app window opened by `account`'s view.
pub(crate) fn popup_label(account: Uuid, number: u64) -> String {
    format!("{}{number}", popup_label_prefix(account))
}

/// Start of the labels of all in-app windows of `account` (the trailing dash keeps it from
/// matching another account).
pub(crate) fn popup_label_prefix(account: Uuid) -> String {
    format!("{POPUP_LABEL_PREFIX}{}-", account.simple())
}

/// True if `label` is the service view or an in-app window of `account`.
pub(crate) fn belongs_to_account(label: &str, account: Uuid) -> bool {
    label == view_label(account) || label.starts_with(&popup_label_prefix(account))
}

/// The account of a service view or an in-app window by its `label`; None for other webviews.
pub(crate) fn account_of_label(label: &str) -> Option<Uuid> {
    let id = label
        .strip_prefix(VIEW_LABEL_PREFIX)
        .or_else(|| label.strip_prefix(POPUP_LABEL_PREFIX)?.split('-').next())?;
    Uuid::try_parse(id).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_label_uses_simple_uuid() {
        let id = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
        assert_eq!(view_label(id), "svc-0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn popup_label_numbers_the_windows_of_an_account() {
        let id = Uuid::from_u128(0xab);
        assert_eq!(
            popup_label(id, 3),
            "popup-000000000000000000000000000000ab-3"
        );
    }

    #[test]
    fn view_account_reads_service_view_labels_only() {
        let account = Uuid::from_u128(0x2a);
        assert_eq!(view_account(&view_label(account)), Some(account));
        assert_eq!(view_account("ui"), None);
        assert_eq!(view_account(&popup_label(account, 1)), None);
        assert_eq!(view_account("svc-nonsense"), None);
    }

    #[test]
    fn account_labels_do_not_match_other_accounts() {
        let (first, second) = (Uuid::from_u128(1), Uuid::from_u128(2));
        assert!(belongs_to_account(&view_label(first), first));
        assert!(belongs_to_account(&popup_label(first, 7), first));
        assert!(!belongs_to_account(&view_label(second), first));
        assert!(!belongs_to_account(&popup_label(second, 7), first));
        assert!(!belongs_to_account("ui", first));
    }

    #[test]
    fn account_is_read_from_view_and_popup_labels() {
        let id = Uuid::from_u128(0xab);
        assert_eq!(account_of_label(&format!("svc-{}", id.simple())), Some(id));
        assert_eq!(
            account_of_label(&format!("popup-{}-3", id.simple())),
            Some(id)
        );
        assert_eq!(account_of_label("ui"), None);
        assert_eq!(account_of_label("svc-nonsense"), None);
    }
}
