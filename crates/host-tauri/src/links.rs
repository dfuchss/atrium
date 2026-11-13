//! New-window policy of service views (and, recursively, of the windows they open).
//!
//! The decision is synchronous inside wry's new-window callback (main thread):
//! - `window.open` with an explicit size (`NewWindowFeatures::size()`) is a sign-in popup
//!   (OAuth/SSO): always an in-app window, so `window.opener` and `postMessage` keep working.
//! - A URL whose host is in the account's `app_hosts` also opens as in-app window.
//! - Anything else follows the account's [`LinkTarget`]: the system browser
//!   (`Deny` + [`Input::ExternalLink`]) or an in-app window of 1100x800.
//!
//! Popup/app windows get the same navigation allow-list ([`domain::nav_allowed`]) and media
//! permission ([`domain::ViewPolicy::allow_media`], scoped to the service's own hosts by
//! [`ViewSpec::media_allowed_at`]) as the account's view.
//!
//! Never `NewWindowResponse::Allow`: wry would create a bare native window without any of
//! our handlers (seen with Rainbow opening its own site).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use domain::{
    Input, ViewPolicy, ViewSpec, host_matches_any, nav_allowed, redact_url, window_title,
};
use protocol::LinkTarget;
use tauri::webview::{
    NewWindowFeatures, NewWindowResponse, PageLoadEvent, PageLoadPayload, PermissionKind,
    PermissionResponse,
};
use tauri::{AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use uuid::Uuid;

use crate::{Feed, Logger};

/// Size of windows opened for `LinkTarget::AppWindow`.
pub(crate) const APP_WINDOW_SIZE: (f64, f64) = (1100.0, 800.0);

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

/// wry does not implement `WebKit`'s `webViewDidClose`, so a popup's own `window.close()` (end of
/// an OAuth flow) would do nothing. This wrapper records the call; [`watch_close`] polls it.
const CLOSE_HOOK: &str = "(()=>{if(window.__msgrCloseHook)return;window.__msgrCloseHook=1;const c=window.close.bind(window);window.close=function(){window.__msgrClosed=true;try{c()}catch(e){}};})();";

/// Label of an in-app window opened by `account`'s view.
pub(crate) fn popup_label(account: Uuid, number: u64) -> String {
    format!("popup-{}-{number}", account.simple())
}

/// Camera, microphone and screen capture are granted only with `media_allowed`, else denied;
/// everything else gets the engine's default handling.
pub(crate) fn permission_response(kind: PermissionKind, media_allowed: bool) -> PermissionResponse {
    match kind {
        PermissionKind::Camera | PermissionKind::Microphone | PermissionKind::DisplayCapture => {
            if media_allowed {
                PermissionResponse::Allow
            } else {
                PermissionResponse::Deny
            }
        }
        _ => PermissionResponse::Default,
    }
}

/// Navigation hook of service views and their windows: [`nav_allowed`], denials are logged.
/// `mailto:` is never loaded in the view but handed to the core as an external link (mail app).
pub(crate) fn navigation_hook(
    account: Uuid,
    logger: Logger,
    feed: Feed,
) -> impl Fn(&Url) -> bool + Send + 'static {
    move |url| {
        let allowed = nav_allowed(url);
        if allowed {
            logger.log_navigation(
                account,
                &format!("[{account}] navigation {}", redact_url(url.as_str())),
            );
        } else if url.scheme() == "mailto" {
            logger.log(&format!("[{account}] mailto link -> mail app"));
            feed(Input::ExternalLink {
                account,
                url: url.to_string(),
            });
        } else {
            logger.log_navigation(
                account,
                &format!(
                    "[{account}] navigation DENIED (scheme) {}",
                    redact_url(url.as_str())
                ),
            );
        }
        allowed
    }
}

/// Permission hook of service views and their windows (reads the current policy). wry drops
/// the requesting origin, so media is scoped by the page currently shown in the main frame.
pub(crate) fn permission_hook<R: Runtime>(
    spec: Arc<ViewSpec>,
    policies: ViewPolicies,
    logger: Logger,
    what: &'static str,
) -> impl Fn(tauri::Webview<R>, PermissionKind) -> PermissionResponse + Send + Sync + 'static {
    move |webview, kind| {
        let account = spec.account;
        let page = webview.url().ok();
        let host = page.as_ref().and_then(Url::host_str).unwrap_or("-");
        let media_allowed =
            spec.media_allowed_at(policy_of(&policies, account).allow_media, page.as_ref());
        let response = permission_response(kind, media_allowed);
        logger.log(&format!(
            "[{account}] {what} permission {kind} at {host} -> {response:?}"
        ));
        response
    }
}

/// New-window decisions of one account's view and of the windows it opens.
pub(crate) struct LinkPolicy<R: Runtime> {
    pub(crate) app: AppHandle<R>,
    pub(crate) spec: Arc<ViewSpec>,
    pub(crate) policies: ViewPolicies,
    pub(crate) popup_counter: Arc<AtomicU64>,
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

    pub(crate) fn on_new_window(
        self: &Arc<Self>,
        url: &Url,
        features: NewWindowFeatures,
    ) -> NewWindowResponse<R> {
        let account = self.spec.account;
        let shown_url = redact_url(url.as_str());
        let kind = if self.spec.is_internal(url) {
            "internal"
        } else {
            "external"
        };
        if let Some(size) = features.size() {
            self.logger.log(&format!(
                "[{account}] new window {shown_url} ({kind}, {}x{}) -> sign-in popup",
                size.width, size.height
            ));
            return self.open_window(url, features, None);
        }
        let policy = policy_of(&self.policies, account);
        if url.host_str().is_some_and(|host| policy.is_app_host(host)) {
            self.logger.log(&format!(
                "[{account}] new window {shown_url} ({kind}) -> app window (app host)"
            ));
            return self.open_window(url, features, Some(APP_WINDOW_SIZE));
        }
        match policy.link_target {
            LinkTarget::SystemBrowser => {
                self.logger.log(&format!(
                    "[{account}] new window {shown_url} ({kind}) -> system browser"
                ));
                (self.feed)(Input::ExternalLink {
                    account,
                    url: url.to_string(),
                });
                NewWindowResponse::Deny
            }
            LinkTarget::AppWindow => {
                self.logger.log(&format!(
                    "[{account}] new window {shown_url} ({kind}) -> app window"
                ));
                self.open_window(url, features, Some(APP_WINDOW_SIZE))
            }
        }
    }

    /// Creates the in-app window `WebKit` loads the request into. Runs on the main thread
    /// (inside wry's callback), where `build()` creates the window synchronously.
    fn open_window(
        self: &Arc<Self>,
        url: &Url,
        features: NewWindowFeatures,
        size: Option<(f64, f64)>,
    ) -> NewWindowResponse<R> {
        let account = self.spec.account;
        let shown_url = redact_url(url.as_str());
        let label = popup_label(
            account,
            self.popup_counter.fetch_add(1, Ordering::Relaxed) + 1,
        );
        let popup_policy = self.clone();
        let title_on_change = Arc::new(PopupTitle::new(self.clone()));
        let title_on_load = title_on_change.clone();
        let blank = Url::parse("about:blank").expect("valid URL");
        let mut builder = WebviewWindowBuilder::new(&self.app, &label, WebviewUrl::External(blank))
            // Requested size/position and, on macOS, the opener's WKWebViewConfiguration, which
            // WebKit requires for the returned view (it also carries the opener's data store).
            .window_features(features)
            .data_store_identifier(self.spec.store_id)
            .title(self.window_title_for("", url))
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
        if let Some((width, height)) = size {
            builder = builder.inner_size(width, height).center();
        }
        if let Some(user_agent) = &self.spec.user_agent {
            builder = builder.user_agent(user_agent);
        }
        match builder.build() {
            Ok(window) => {
                self.logger.log(&format!(
                    "[{account}] opened window {label} for {shown_url}"
                ));
                watch_close(self.app.clone(), label, &self.logger);
                NewWindowResponse::Create { window }
            }
            Err(error) => {
                self.logger.log(&format!(
                    "[{account}] cannot open window for {shown_url}: {error}"
                ));
                NewWindowResponse::Deny
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
    let spawned = std::thread::Builder::new()
        .name(format!("watch-{label}"))
        .spawn(move || {
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
    if let Err(error) = spawned {
        logger.log(&format!("cannot watch window for window.close(): {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_permissions() {
        let id = Uuid::from_u128(0xab);
        assert_eq!(
            popup_label(id, 3),
            "popup-000000000000000000000000000000ab-3"
        );
        for kind in [
            PermissionKind::Camera,
            PermissionKind::Microphone,
            PermissionKind::DisplayCapture,
        ] {
            assert!(matches!(
                permission_response(kind, true),
                PermissionResponse::Allow
            ));
            assert!(matches!(
                permission_response(kind, false),
                PermissionResponse::Deny
            ));
        }
        assert!(matches!(
            permission_response(PermissionKind::Geolocation, true),
            PermissionResponse::Default
        ));
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
