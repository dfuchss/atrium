//! The in-app windows an account's view opens (sign-in popups and app windows): their quota,
//! how they are built, and their titles.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};

use domain::{WindowQuota, WindowRefusal, host_matches_any, redact_url, window_title};
use tauri::webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent, PageLoadPayload};
use tauri::{Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use uuid::Uuid;

use super::close_hook::{CLOSE_HOOK, watch_close};
use super::{LinkPolicy, policy_of};
use crate::labels::popup_label;
use crate::logger::now_ms;
use crate::native;
use crate::page_hooks::{navigation_hook, permission_hook};
use crate::webviews::popup_labels;

/// Size of windows opened for `LinkTarget::AppWindow`.
const APP_WINDOW_SIZE: (f64, f64) = (1100.0, 800.0);

/// How an in-app window opens.
#[derive(Clone, Copy)]
pub(super) enum Opening {
    /// At the size and place the page asked for (a sign-in popup).
    Requested,
    /// At [`APP_WINDOW_SIZE`], centred.
    AppWindow,
    /// Hidden, until it is known whether the page asked for a size (Linux, see
    /// `LinkPolicy::system_browser`).
    #[cfg(target_os = "linux")]
    Undecided,
}

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

    /// In-app windows the account has open.
    pub(super) fn open_windows(&self) -> usize {
        popup_labels(&self.app, self.spec.account).len()
    }

    /// True if the account may open another window besides the `open_now` it has; logs the
    /// refusal otherwise.
    pub(super) fn admit_window(&self, shown_url: &str, open_now: usize) -> bool {
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
    pub(super) fn open_window(
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
    pub(super) fn create_window(
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
