//! Linux: a new-window request carries no size, so a window bound for the system browser
//! opens hidden until `WebKitGTK` tells the size its page asked for, which makes it a sign-in
//! popup.

use std::sync::Arc;
use std::time::Duration;

use domain::redact_url;
use tauri::webview::{NewWindowFeatures, NewWindowResponse};
use tauri::{Runtime, Url, WebviewWindow};

use super::LinkPolicy;
use super::popups::Opening;
use crate::logger::spawn_logged;
use crate::native;

/// How long a hidden window may wait for its page's requested size before it is closed: a page
/// whose view never reports ready to show must not leave an invisible window behind.
const UNDECIDED_TIMEOUT: Duration = Duration::from_secs(5);

impl<R: Runtime> LinkPolicy<R> {
    /// A request for the system browser, unless it is a sign-in popup: wry's request carries no
    /// size on Linux, `WebKitGTK` tells it once the new window is ready to show. So the window
    /// opens hidden (not counted against the quota yet) and [`Self::decide_when_shown`] makes it
    /// a sign-in popup or closes it and hands its URL to the system browser.
    pub(super) fn system_browser(
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
}
