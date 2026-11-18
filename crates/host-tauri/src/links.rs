//! New-window policy of service views (and, recursively, of the windows they open).
//!
//! The decision is synchronous inside wry's new-window callback (main thread):
//! - `window.open` with an explicit size (`NewWindowFeatures::size()`) is a sign-in popup
//!   (OAuth/SSO): always an in-app window, so `window.opener` and `postMessage` keep working.
//!   On Linux the request carries no size: a window bound for the system browser opens hidden
//!   until `WebKitGTK` tells the size it asked for (see `LinkPolicy::system_browser`).
//! - A URL whose host is in the account's `app_hosts` also opens as in-app window.
//! - Anything else follows the account's [`LinkTarget`]: the system browser
//!   (`Deny` + [`Input::ExternalLink`]) or an in-app window of 1100x800.
//!
//! Popup/app windows get the same navigation and permission hooks (`page_hooks`) and
//! JavaScript dialogs (`dialogs`) as the account's view.
//!
//! Never `NewWindowResponse::Allow`: wry would create a bare native window without any of
//! our handlers (seen with Rainbow opening its own site).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use domain::{
    Input, ViewPolicy, ViewSpec, WindowQuota, WindowRefusal, host_matches_any, nav_allowed,
    redact_url, window_title,
};
use protocol::LinkTarget;
use tauri::webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent, PageLoadPayload};
use tauri::{AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use uuid::Uuid;

use crate::labels::popup_label;
use crate::logger::now_ms;
use crate::logger::spawn_logged;
use crate::native;
use crate::page_hooks::{navigation_hook, permission_hook};
use crate::webviews::popup_labels;
use crate::{Feed, Logger};

/// How long a hidden window may wait for its page's requested size before it is closed: a page
/// whose view never reports ready to show must not leave an invisible window behind.
#[cfg(target_os = "linux")]
const UNDECIDED_TIMEOUT: Duration = Duration::from_secs(5);

/// Size of windows opened for `LinkTarget::AppWindow`.
pub(crate) const APP_WINDOW_SIZE: (f64, f64) = (1100.0, 800.0);

/// How an in-app window opens.
#[derive(Clone, Copy)]
enum Opening {
    /// At the size and place the page asked for (a sign-in popup).
    Requested,
    /// At [`APP_WINDOW_SIZE`], centred.
    AppWindow,
    /// Hidden, until it is known whether the page asked for a size (Linux, see
    /// `LinkPolicy::system_browser`).
    #[cfg(target_os = "linux")]
    Undecided,
}

/// Effective view policy per account, written by the actor (`set_view_policy`), read in the
/// new-window and permission callbacks. Missing entry = system browser, no media.
pub(crate) type ViewPolicies = Arc<Mutex<HashMap<Uuid, ViewPolicy>>>;

/// Current policy of `account` (default: system browser, media denied).
pub(crate) fn policy_of(policies: &ViewPolicies, account: Uuid) -> ViewPolicy {
    policies
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&account)
        .cloned()
        .unwrap_or(ViewPolicy {
            link_target: LinkTarget::SystemBrowser,
            allow_media: false,
            app_hosts: Vec::new(),
        })
}

/// wry cannot close a popup on its own `window.close()` (end of an OAuth flow): on macOS it does
/// not implement `WebKit`'s `webViewDidClose`, on Linux it destroys the webview but leaves its
/// window (and a service view with a single page in its history may close itself). So this
/// wrapper only records the call; [`watch_close`] polls it in popups and closes their window.
/// Nothing polls a service view, where the call does nothing.
pub(crate) const CLOSE_HOOK: &str = "(()=>{if(window.__msgrCloseHook)return;window.__msgrCloseHook=1;window.close=function(){window.__msgrClosed=true;};})();";

/// Most in-app windows an account may have open at once.
const MAX_OPEN_WINDOWS: usize = 5;
/// Most in-app windows an account may open within [`OPEN_WINDOWS_PERIOD_MS`].
const MAX_WINDOWS_PER_PERIOD: usize = 3;
const OPEN_WINDOWS_PERIOD_MS: u64 = 10_000;

/// Window admission shared by all accounts' views.
pub(crate) type PopupQuota = Arc<Mutex<WindowQuota<Uuid>>>;

/// A quota with the limits above.
pub(crate) fn new_popup_quota() -> PopupQuota {
    Arc::new(Mutex::new(WindowQuota::new(
        MAX_OPEN_WINDOWS,
        MAX_WINDOWS_PER_PERIOD,
        OPEN_WINDOWS_PERIOD_MS,
    )))
}

/// What a new-window request may become before the account's link policy decides.
#[derive(Debug, PartialEq, Eq)]
enum NewWindowGate {
    /// `mailto:` goes to the core as an external link (mail app), as in the navigation hook.
    MailApp,
    /// Not a page a window may load ([`nav_allowed`]), e.g. Atrium's own UI hosts.
    Denied,
    Open,
}

fn new_window_gate(url: &Url) -> NewWindowGate {
    if url.scheme() == "mailto" {
        NewWindowGate::MailApp
    } else if nav_allowed(url) {
        NewWindowGate::Open
    } else {
        NewWindowGate::Denied
    }
}

/// Where a new-window request that passed the [`NewWindowGate`] goes.
#[derive(Debug, PartialEq, Eq)]
enum NewWindowRoute<S> {
    /// An in-app window of the requested size `S`.
    SignInPopup(S),
    /// An in-app window, as the URL's host is one of the account's app hosts.
    AppHost,
    /// An in-app window, as the account's links open in one ([`LinkTarget::AppWindow`]).
    AppWindow,
    /// The system browser ([`LinkTarget::SystemBrowser`]); on Linux it may still turn out to be
    /// a sign-in popup (see `LinkPolicy::system_browser`).
    SystemBrowser,
}

/// The route of a request for `url`, with the `size` the page asked for, under `policy`: a
/// requested size makes a sign-in popup, an app host an app window, anything else follows the
/// account's link target.
fn new_window_route<S>(url: &Url, size: Option<S>, policy: &ViewPolicy) -> NewWindowRoute<S> {
    if let Some(size) = size {
        return NewWindowRoute::SignInPopup(size);
    }
    if url.host_str().is_some_and(|host| policy.is_app_host(host)) {
        return NewWindowRoute::AppHost;
    }
    match policy.link_target {
        LinkTarget::SystemBrowser => NewWindowRoute::SystemBrowser,
        LinkTarget::AppWindow => NewWindowRoute::AppWindow,
    }
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
    /// Window title for `page` (see [`window_title`]): the account's internal and app hosts
    /// count as its own, as does the blank page a window starts with.
    fn window_title_for(&self, page_title: &str, page: &Url) -> String {
        let own_host = page.scheme() == "about"
            || page.host_str().is_some_and(|host| {
                host_matches_any(&self.spec.internal_hosts, host)
                    || policy_of(&self.policies, self.spec.account).is_app_host(host)
            });
        window_title(page_title, page, own_host)
    }

    /// How `url` is described in the log: one of the service's own pages or not.
    fn kind_of(&self, url: &Url) -> &'static str {
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
    fn open_in_browser(&self, url: &Url) {
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

    /// A request for the system browser, unless it is a sign-in popup: wry's request carries no
    /// size on Linux, `WebKitGTK` tells it once the new window is ready to show. So the window
    /// opens hidden (not counted against the quota yet) and [`Self::decide_when_shown`] makes it
    /// a sign-in popup or closes it and hands its URL to the system browser.
    #[cfg(target_os = "linux")]
    fn system_browser(
        self: &Arc<Self>,
        url: &Url,
        features: NewWindowFeatures,
    ) -> NewWindowResponse<R> {
        let Some(window) = self.create_window(url, features, Opening::Undecided) else {
            return NewWindowResponse::Deny;
        };
        let (policy, target, page) = (self.clone(), window.clone(), url.clone());
        let watched = native::on_requested_size(&window, move |size| {
            policy.decide_when_shown(&target, &page, size);
        });
        match watched {
            Ok(()) => {
                self.destroy_if_undecided_after_timeout(&window);
                NewWindowResponse::Create { window }
            }
            Err(error) => {
                self.logger.log(&format!(
                    "[{}] cannot learn the size of window {}: {error}",
                    self.spec.account,
                    window.label()
                ));
                self.decide_when_shown(&window, url, None);
                NewWindowResponse::Deny
            }
        }
    }

    /// Destroys the hidden `window` if it is still hidden after [`UNDECIDED_TIMEOUT`]:
    /// [`Self::decide_when_shown`] shows a popup or destroys the window, so a hidden one never
    /// heard that its view is ready to show.
    #[cfg(target_os = "linux")]
    fn destroy_if_undecided_after_timeout(&self, window: &WebviewWindow<R>) {
        let (window, logger) = (window.clone(), self.logger.clone());
        let account = self.spec.account;
        spawn_logged("undecided-window", &self.logger, move || {
            std::thread::sleep(UNDECIDED_TIMEOUT);
            if window.is_visible().is_ok_and(|visible| !visible) {
                logger.log(&format!(
                    "[{account}] window {} never became ready to show, closed",
                    window.label()
                ));
                let _ = window.destroy();
            }
        });
    }

    /// Shows the hidden `window` for `url` as sign-in popup of `size`, the size its page asked
    /// for, if the quota admits it; without a size it is a link: the window closes and `url`
    /// goes to the system browser.
    #[cfg(target_os = "linux")]
    fn decide_when_shown(
        &self,
        window: &WebviewWindow<R>,
        url: &Url,
        size: Option<tauri::LogicalSize<f64>>,
    ) {
        let Some(size) = size else {
            let _ = window.destroy();
            self.open_in_browser(url);
            return;
        };
        let shown_url = redact_url(url.as_str());
        // The hidden window itself does not count.
        let open_besides = self.open_windows().saturating_sub(1);
        if !self.admit_window(&shown_url, open_besides) {
            let _ = window.destroy();
            return;
        }
        self.logger.log(&format!(
            "[{}] window {} for {shown_url} ({}, {}x{}) -> sign-in popup",
            self.spec.account,
            window.label(),
            self.kind_of(url),
            size.width,
            size.height
        ));
        let _ = window.set_size(size);
        let _ = window.center();
        let _ = window.show();
        let _ = window.set_focus();
    }

    /// In-app windows the account has open.
    fn open_windows(&self) -> usize {
        popup_labels(&self.app, self.spec.account).len()
    }

    /// True if the account may open another window besides the `open_now` it has; logs the
    /// refusal otherwise.
    fn admit_window(&self, shown_url: &str, open_now: usize) -> bool {
        let account = self.spec.account;
        let admitted = self
            .popup_quota
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .admit(account, open_now, now_ms());
        let Err(refusal) = admitted else {
            return true;
        };
        let reason = match refusal {
            WindowRefusal::TooManyOpen => "too many windows open",
            WindowRefusal::TooFast => "opening windows too fast",
        };
        self.logger.log(&format!(
            "[{account}] window for {shown_url} refused: {reason}"
        ));
        false
    }

    /// The in-app window `label` for `url`, with the hooks of the account's windows.
    fn build_window(
        self: &Arc<Self>,
        url: &Url,
        label: &str,
        features: NewWindowFeatures,
        opening: Opening,
    ) -> tauri::Result<WebviewWindow<R>> {
        let account = self.spec.account;
        let popup_policy = self.clone();
        let title_on_change = Arc::new(PopupTitle::new(self.clone()));
        let title_on_load = title_on_change.clone();
        let blank = Url::parse("about:blank").expect("valid URL");
        let builder = WebviewWindowBuilder::new(&self.app, label, WebviewUrl::External(blank));
        // Like the account's view: `window.open` only from a click, page drag and drop.
        let builder = native::prepare_popup(builder, &features)
            // Requested size/position and, on macOS, the opener's WKWebViewConfiguration, which
            // WebKit requires for the returned view (it also carries the opener's data store);
            // on Linux the opener as related view; on Windows the opener's WebView2
            // environment, which WebView2 requires (it also carries the opener's data folder).
            .window_features(features);
        let mut builder = native::isolate_window(builder, &self.app, self.spec.store_id)?
            .title(self.window_title_for("", url))
            // Where popups run their opener's scripts instead, the service view carries it.
            .initialization_script(CLOSE_HOOK)
            .on_new_window(move |url, features| popup_policy.on_new_window(&url, features))
            .on_document_title_changed(move |window, title| {
                title_on_change.title_changed(&window, title);
            })
            .on_page_load(move |window, payload| title_on_load.page_loading(&window, &payload))
            .on_navigation(navigation_hook(
                account,
                self.logger.clone(),
                self.feed.clone(),
            ))
            .on_permission_request(permission_hook(
                self.spec.clone(),
                self.policies.clone(),
                self.logger.clone(),
                "popup",
            ));
        match opening {
            Opening::Requested => {}
            Opening::AppWindow => {
                let (width, height) = APP_WINDOW_SIZE;
                builder = builder.inner_size(width, height).center();
            }
            #[cfg(target_os = "linux")]
            Opening::Undecided => {
                let (width, height) = native::UNDECIDED_WINDOW_SIZE;
                builder = builder.inner_size(width, height).visible(false);
            }
        }
        if let Some(user_agent) = &self.spec.user_agent {
            builder = builder.user_agent(user_agent);
        }
        builder.build()
    }

    /// Opens the in-app window `WebKit` loads the request into, if the quota admits it.
    fn open_window(
        self: &Arc<Self>,
        url: &Url,
        features: NewWindowFeatures,
        opening: Opening,
    ) -> NewWindowResponse<R> {
        if !self.admit_window(&redact_url(url.as_str()), self.open_windows()) {
            return NewWindowResponse::Deny;
        }
        match self.create_window(url, features, opening) {
            Some(window) => NewWindowResponse::Create { window },
            None => NewWindowResponse::Deny,
        }
    }

    /// Creates an in-app window for `url` with the account's dialogs and `window.close()`; logs
    /// a failure. Runs on the main thread (inside wry's callback), where `build()` creates the
    /// window synchronously.
    fn create_window(
        self: &Arc<Self>,
        url: &Url,
        features: NewWindowFeatures,
        opening: Opening,
    ) -> Option<WebviewWindow<R>> {
        let account = self.spec.account;
        let shown_url = redact_url(url.as_str());
        let label = popup_label(
            account,
            self.popup_counter.fetch_add(1, Ordering::Relaxed) + 1,
        );
        match self.build_window(url, &label, features, opening) {
            Ok(window) => {
                self.logger.log(&format!(
                    "[{account}] opened window {label} for {shown_url}"
                ));
                native::attach_dialogs(window.as_ref(), &self.logger);
                watch_close(self.app.clone(), label, &self.logger);
                Some(window)
            }
            Err(error) => {
                self.logger.log(&format!(
                    "[{account}] cannot open window for {shown_url}: {error}"
                ));
                None
            }
        }
    }
}

/// Keeps a popup/app window's title at `"<page title> — <host>"` while its page and title
/// change, so the shown host is always the one actually loaded.
struct PopupTitle<R: Runtime> {
    policy: Arc<LinkPolicy<R>>,
    page_title: Mutex<String>,
}

impl<R: Runtime> PopupTitle<R> {
    fn new(policy: Arc<LinkPolicy<R>>) -> PopupTitle<R> {
        PopupTitle {
            policy,
            page_title: Mutex::default(),
        }
    }

    fn title_changed(&self, window: &WebviewWindow<R>, title: String) {
        if let Ok(page) = window.url() {
            let _ = window.set_title(&self.policy.window_title_for(&title, &page));
        }
        *self
            .page_title
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = title;
    }

    /// A new page starts without the previous page's title; its own follows as title change.
    fn page_loading(&self, window: &WebviewWindow<R>, payload: &PageLoadPayload<'_>) {
        let mut page_title = self
            .page_title
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let PageLoadEvent::Started = payload.event() {
            page_title.clear();
        }
        let _ = window.set_title(&self.policy.window_title_for(&page_title, payload.url()));
    }
}

/// Closes the window `label` once its page called `window.close()` (see [`CLOSE_HOOK`]);
/// ends when the window is gone.
fn watch_close<R: Runtime>(app: AppHandle<R>, label: String, logger: &Logger) {
    spawn_logged(&format!("watch-{label}"), logger, move || {
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let Some(window) = app.get_webview_window(&label) else {
                break;
            };
            let target = window.clone();
            let _ = window.eval_with_callback("!!window.__msgrClosed", move |raw| {
                if raw == "true" {
                    let _ = target.close();
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_hook_only_records_the_call() {
        assert!(CLOSE_HOOK.contains("window.close=function(){window.__msgrClosed=true;}"));
        assert!(!CLOSE_HOOK.contains(".close("));
    }

    #[test]
    fn new_windows_send_mail_links_to_the_mail_app_and_deny_the_app_ui() {
        let gate = |url: &str| new_window_gate(&Url::parse(url).unwrap());
        assert_eq!(gate("mailto:a@b.org"), NewWindowGate::MailApp);
        assert_eq!(gate("https://example.org/"), NewWindowGate::Open);
        assert_eq!(gate("about:blank"), NewWindowGate::Open);
        assert_eq!(gate("http://tauri.localhost/"), NewWindowGate::Denied);
        assert_eq!(gate("https://ipc.localhost/x"), NewWindowGate::Denied);
        assert_eq!(gate("file:///C:/Windows/win.ini"), NewWindowGate::Denied);
        assert_eq!(gate("javascript:alert(1)"), NewWindowGate::Denied);
    }

    #[test]
    fn new_windows_go_by_size_then_app_host_then_link_target() {
        let policy = |link_target| ViewPolicy {
            link_target,
            allow_media: false,
            app_hosts: vec!["*.sso.example".into()],
        };
        let route = |url: &str, size: Option<(u32, u32)>, link_target| {
            new_window_route(&Url::parse(url).unwrap(), size, &policy(link_target))
        };
        let (link, app_host) = ("https://example.org/", "https://login.sso.example/");
        for link_target in [LinkTarget::SystemBrowser, LinkTarget::AppWindow] {
            assert_eq!(
                route(link, Some((500, 600)), link_target),
                NewWindowRoute::SignInPopup((500, 600))
            );
            assert_eq!(route(app_host, None, link_target), NewWindowRoute::AppHost);
        }
        assert_eq!(
            route(link, None, LinkTarget::SystemBrowser),
            NewWindowRoute::SystemBrowser
        );
        assert_eq!(
            route(link, None, LinkTarget::AppWindow),
            NewWindowRoute::AppWindow
        );
        assert_eq!(
            route("about:blank", None, LinkTarget::SystemBrowser),
            NewWindowRoute::SystemBrowser
        );
    }

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
