//! New-window policy of service views (and, recursively, of the windows they open).
//!
//! The decision is synchronous inside wry's new-window callback (main thread):
//! - `window.open` with an explicit size (`NewWindowFeatures::size()`) is a sign-in popup
//!   (OAuth/SSO): always an in-app window, so `window.opener` and `postMessage` keep working.
//!   On Linux the request carries no size: a window bound for the system browser opens hidden
//!   until `WebKitGTK` tells the size it asked for (see `LinkPolicy::system_browser`).
//! - A URL whose host is in the account's `app_hosts` also opens as in-app window.
//! - Anything else follows the account's [`LinkTarget`](protocol::LinkTarget): the system
//!   browser (`Deny` + [`Input::ExternalLink`]) or an in-app window of 1100x800.
//!
//! The domain decides ([`new_window_gate`], [`new_window_route`]); this module carries it out,
//! `popups` opens the in-app windows, `undecided` holds the hidden windows of Linux, and
//! `close_hook` closes a popup on its page's `window.close()`.
//!
//! Popup/app windows get the same navigation and permission hooks (`page_hooks`) and
//! JavaScript dialogs (`dialogs`) as the account's view.
//!
//! Never `NewWindowResponse::Allow`: wry would create a bare native window without any of
//! our handlers (seen with Rainbow opening its own site).

mod close_hook;
mod popups;
#[cfg(target_os = "linux")]
mod undecided;

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use domain::{
    Input, NewWindowGate, NewWindowRoute, ViewPolicy, ViewSpec, new_window_gate, new_window_route,
    redact_url,
};
use fs_util::lock_ignoring_poison;
use tauri::webview::{NewWindowFeatures, NewWindowResponse};
use tauri::{AppHandle, Runtime, Url};
use uuid::Uuid;

use crate::{Feed, Logger};
pub(crate) use close_hook::CLOSE_HOOK;
use popups::Opening;
pub(crate) use popups::{PopupQuota, new_popup_quota};

/// Effective view policy per account, written by the actor (`set_view_policy`), read in the
/// new-window and permission callbacks. Missing entry = system browser, no media.
pub(crate) type ViewPolicies = Arc<Mutex<HashMap<Uuid, ViewPolicy>>>;

/// Current policy of `account` (the default one if it has none yet).
pub(crate) fn policy_of(policies: &ViewPolicies, account: Uuid) -> ViewPolicy {
    lock_ignoring_poison(policies)
        .get(&account)
        .cloned()
        .unwrap_or_default()
}

/// The new-window decisions of one account's view and of the windows it opens (shared by all
/// of them), with what it takes to open the in-app windows: the account's spec and current
/// policy, the window counter and quota, and the core's feed.
pub(crate) struct LinkPolicy<R: Runtime> {
    pub(crate) app: AppHandle<R>,
    pub(crate) spec: Arc<ViewSpec>,
    pub(crate) policies: ViewPolicies,
    pub(crate) popup_counter: Arc<AtomicU64>,
    pub(crate) popup_quota: PopupQuota,
    pub(crate) feed: Feed,
    pub(crate) logger: Logger,
}

impl<R: Runtime> LinkPolicy<R> {
    /// How `url` is described in the log: one of the service's own pages or not.
    pub(super) fn kind_of(&self, url: &Url) -> &'static str {
        if self.spec.is_internal(url) {
            "internal"
        } else {
            "external"
        }
    }

    pub(crate) fn on_new_window(
        self: &Arc<Self>,
        url: &Url,
        features: NewWindowFeatures,
    ) -> NewWindowResponse<R> {
        let account = self.spec.account;
        let shown_url = redact_url(url.as_str());
        match new_window_gate(url) {
            NewWindowGate::MailApp => {
                self.open_in_browser(url);
                return NewWindowResponse::Deny;
            }
            NewWindowGate::Denied => {
                self.logger
                    .log(&format!("[{account}] new window {shown_url} -> denied"));
                return NewWindowResponse::Deny;
            }
            NewWindowGate::Open => {}
        }
        let kind = self.kind_of(url);
        let policy = policy_of(&self.policies, account);
        match new_window_route(url, features.size(), &policy) {
            NewWindowRoute::SignInPopup(size) => {
                self.logger.log(&format!(
                    "[{account}] new window {shown_url} ({kind}, {}x{}) -> sign-in popup",
                    size.width, size.height
                ));
                self.open_window(url, features, Opening::Requested)
            }
            NewWindowRoute::AppHost => {
                self.logger.log(&format!(
                    "[{account}] new window {shown_url} ({kind}) -> app window (app host)"
                ));
                self.open_window(url, features, Opening::AppWindow)
            }
            NewWindowRoute::AppWindow => {
                self.logger.log(&format!(
                    "[{account}] new window {shown_url} ({kind}) -> app window"
                ));
                self.open_window(url, features, Opening::AppWindow)
            }
            NewWindowRoute::SystemBrowser => self.system_browser(url, features),
        }
    }

    /// Hands `url` to the system browser (through the core).
    pub(super) fn open_in_browser(&self, url: &Url) {
        let account = self.spec.account;
        self.logger.log(&format!(
            "[{account}] new window {} ({}) -> system browser",
            redact_url(url.as_str()),
            self.kind_of(url)
        ));
        (self.feed)(Input::ExternalLink {
            account,
            url: url.to_string(),
        });
    }

    /// A request without a size: a link for the system browser. macOS and Windows tell a
    /// sign-in popup's size with the request.
    #[cfg(not(target_os = "linux"))]
    fn system_browser(
        self: &Arc<Self>,
        url: &Url,
        _features: NewWindowFeatures,
    ) -> NewWindowResponse<R> {
        self.open_in_browser(url);
        NewWindowResponse::Deny
    }
}

#[cfg(test)]
mod tests {
    use protocol::LinkTarget;

    use super::*;

    #[test]
    fn policy_defaults_and_updates() {
        let policies = ViewPolicies::default();
        let id = Uuid::from_u128(1);
        assert_eq!(
            policy_of(&policies, id),
            ViewPolicy {
                link_target: LinkTarget::SystemBrowser,
                allow_media: false,
                app_hosts: vec![]
            }
        );
        let set = ViewPolicy {
            link_target: LinkTarget::AppWindow,
            allow_media: true,
            app_hosts: vec!["*.sso.example".into()],
        };
        policies.lock().unwrap().insert(id, set.clone());
        assert_eq!(policy_of(&policies, id), set);
    }
}
