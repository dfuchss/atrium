//! Service views: one child webview per account in the main window, with its native hooks.

use std::sync::{Arc, PoisonError, mpsc};
use std::time::Duration;

use domain::ports::{PortError, PortResult, WebviewHost};
use domain::{Bounds, ViewPolicy, ViewSpec, redact_url};
use protocol::UI_WEBVIEW;
use tauri::utils::config::BackgroundThrottlingPolicy;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, Runtime, Webview, WebviewBuilder, WebviewUrl, Window};
use uuid::Uuid;

use crate::data_stores::delete_data_store;
use crate::downloads::download_hook;
use crate::labels::{popup_label_prefix, view_label};
use crate::links::{CLOSE_HOOK, LinkPolicy};
use crate::logger::spawn_logged;
use crate::native;
use crate::page_hooks::{navigation_hook, permission_hook};
use crate::{Input, TauriHost, port_err};

/// Time `WebKit` gets to delete the browsing data before the view reloads.
const CLEAR_DATA_RELOAD_DELAY: Duration = Duration::from_millis(800);

/// A service view's init script: the service's, then the development script, if any, and, if
/// `popups_run_its_scripts`, the popups' [`CLOSE_HOOK`], which they run only that way.
fn service_view_script(service: &str, extra: Option<&str>, popups_run_its_scripts: bool) -> String {
    let close_hook = popups_run_its_scripts.then_some(CLOSE_HOOK);
    [Some(service), extra, close_hook]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n")
}

/// Labels of the in-app (popup and app) windows `account` currently has.
pub(crate) fn popup_labels<R: Runtime>(app: &AppHandle<R>, account: Uuid) -> Vec<String> {
    let prefix = popup_label_prefix(account);
    app.webview_windows()
        .into_keys()
        .filter(|label| label.starts_with(&prefix))
        .collect()
}

/// Builds the service view on the main thread (also when called from elsewhere; waits for it),
/// where [`native::build_service_view`] has to run.
fn add_service_view<R: Runtime>(
    window: &Window<R>,
    builder: WebviewBuilder<R>,
    store_id: [u8; 16],
    bounds: Bounds,
) -> PortResult<Webview<R>> {
    let (sender, receiver) = mpsc::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let created = native::build_service_view(&target, builder, store_id, bounds);
            let _ = sender.send(created);
        })
        .map_err(port_err)?;
    receiver
        .recv()
        .map_err(|_| PortError("the view was not created".into()))?
}

impl<R: Runtime> TauriHost<R> {
    fn webview(&self, account: Uuid) -> PortResult<Webview<R>> {
        self.app
            .get_webview(&view_label(account))
            .ok_or_else(|| PortError(format!("no view for account {account}")))
    }

    /// The view's init script (see [`service_view_script`]).
    fn init_script(&self, spec: &ViewSpec) -> String {
        service_view_script(
            &spec.init_script,
            self.extra_init_script.as_deref(),
            native::POPUPS_RUN_OPENER_SCRIPTS,
        )
    }

    /// Adds the native hooks of one service view to `builder`.
    fn hook(&self, spec: &ViewSpec, builder: WebviewBuilder<R>) -> WebviewBuilder<R> {
        let account = spec.account;
        let shared_spec = Arc::new(spec.clone());
        let link_policy = Arc::new(LinkPolicy {
            app: self.app.clone(),
            spec: shared_spec.clone(),
            policies: self.policies.clone(),
            popup_counter: self.popup_counter.clone(),
            popup_quota: self.popup_quota.clone(),
            feed: self.feed.clone(),
            logger: self.logger.clone(),
        });
        let (feed_title, feed_load) = (self.feed.clone(), self.feed.clone());
        let (load_logger, load_dialogs) = (self.logger.clone(), self.dialogs.clone());
        builder
            .on_document_title_changed(move |_, title| {
                feed_title(Input::TitleChanged { account, title });
            })
            .on_page_load(move |_, payload| {
                if let PageLoadEvent::Finished = payload.event() {
                    load_logger.log(&format!(
                        "[{account}] loaded {}",
                        redact_url(payload.url().as_str())
                    ));
                    load_dialogs.page_loaded(account);
                    feed_load(Input::PageLoaded {
                        account,
                        url: payload.url().to_string(),
                    });
                }
            })
            // Fires for iframe loads too (no frame info), so it must never open anything
            // externally (sign-in and captcha iframes would end up in the browser); it only
            // allows or denies by scheme (`domain::nav_allowed`).
            .on_navigation(navigation_hook(
                account,
                self.logger.clone(),
                self.feed.clone(),
            ))
            // Never Allow: see `links` (sign-in popups -> in-app window, else the link target).
            .on_new_window(move |url, features| link_policy.on_new_window(&url, features))
            .on_download(download_hook(
                self.app.clone(),
                account,
                self.logger.clone(),
                self.feed.clone(),
            ))
            .on_permission_request(permission_hook(
                shared_spec,
                self.policies.clone(),
                self.logger.clone(),
                "view",
            ))
    }
}

impl<R: Runtime> WebviewHost for TauriHost<R> {
    fn create(&self, spec: &ViewSpec, bounds: Bounds, visible: bool) -> PortResult {
        let label = view_label(spec.account);
        if self.app.get_webview(&label).is_some() {
            return Err(PortError(format!(
                "view for account {} already exists",
                spec.account
            )));
        }
        let builder = WebviewBuilder::new(label, WebviewUrl::External(spec.url.clone()));
        let mut builder = native::isolate_view(builder, &self.app, spec.store_id)
            .map_err(port_err)?
            .initialization_script(self.init_script(spec))
            // Hidden accounts keep running (calls, sockets, timers).
            .background_throttling(BackgroundThrottlingPolicy::Disabled);
        if let Some(user_agent) = &spec.user_agent {
            builder = builder.user_agent(user_agent);
        }
        let builder = self.hook(spec, builder);
        let webview = add_service_view(&self.window()?, builder, spec.store_id, bounds)?;
        native::attach_dialogs(&webview, &self.logger);
        native::observe_web_process_termination(&webview, &self.feed, &self.logger);
        if !visible {
            native::set_visible(&webview, false)?;
        }
        self.logger.log(&format!(
            "[{}] created view {} ({})",
            spec.account,
            webview.label(),
            redact_url(spec.url.as_str())
        ));
        Ok(())
    }

    fn set_bounds(&self, account: Uuid, bounds: Bounds) -> PortResult {
        native::place(&self.webview(account)?, bounds)
    }

    fn set_ui_bounds(&self, bounds: Bounds) -> PortResult {
        let ui = self
            .app
            .get_webview(UI_WEBVIEW)
            .ok_or_else(|| PortError(format!("webview {UI_WEBVIEW:?} not found")))?;
        native::place(&ui, bounds)
    }

    fn show(&self, account: Uuid) -> PortResult {
        native::set_visible(&self.webview(account)?, true)
    }

    fn hide(&self, account: Uuid) -> PortResult {
        native::set_visible(&self.webview(account)?, false)
    }

    fn destroy(&self, account: Uuid) -> PortResult {
        // The windows share the account's data store and login: they go with the view.
        for label in popup_labels(&self.app, account) {
            if let Some(window) = self.app.get_webview_window(&label) {
                // Already closing is fine; the view's own result is what matters.
                let _ = window.close();
            }
        }
        self.webview(account)?.close().map_err(port_err)
    }

    fn reload(&self, account: Uuid) -> PortResult {
        self.webview(account)?.reload().map_err(port_err)
    }

    fn force_reload(&self, account: Uuid) -> PortResult {
        native::force_reload(&self.webview(account)?, &self.logger)
    }

    fn eval(&self, account: Uuid, script: &str) -> PortResult {
        self.webview(account)?.eval(script).map_err(port_err)
    }

    fn poll(&self, account: Uuid, script: &str) -> PortResult {
        let feed = self.feed.clone();
        self.webview(account)?
            .eval_with_callback(script, move |raw| feed(Input::PollResult { account, raw }))
            .map_err(port_err)
    }

    fn set_view_policy(&self, account: Uuid, policy: ViewPolicy) -> PortResult {
        self.policies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(account, policy);
        Ok(())
    }

    fn set_zoom(&self, account: Uuid, zoom: f64) -> PortResult {
        self.webview(account)?.set_zoom(zoom).map_err(port_err)
    }

    fn clear_data(&self, account: Uuid) -> PortResult {
        let webview = self.webview(account)?;
        webview.clear_all_browsing_data().map_err(port_err)?;
        // The deletion is asynchronous; reload once it has most likely finished.
        let logger = self.logger.clone();
        spawn_logged("clear-data", &self.logger, move || {
            std::thread::sleep(CLEAR_DATA_RELOAD_DELAY);
            let result = webview.reload();
            logger.log(&format!(
                "[{account}] cleared browsing data, reload -> {result:?}"
            ));
        });
        Ok(())
    }

    fn current_url(&self, account: Uuid) -> PortResult<String> {
        self.webview(account)?
            .url()
            .map(|url| url.to_string())
            .map_err(port_err)
    }

    fn find_icon(&self, account: Uuid, script: &str) -> PortResult {
        let feed = self.feed.clone();
        let webview = self.webview(account)?;
        let page = webview.clone();
        webview
            .eval_with_callback(script, move |raw| {
                // Read here, not reported by the script: the page could fake its own location.
                let page_url = page.url().ok().map(String::from);
                feed(Input::IconFound {
                    account,
                    raw,
                    page_url,
                });
            })
            .map_err(port_err)
    }

    fn delete_profile(&self, store_id: [u8; 16]) -> PortResult {
        delete_data_store(&self.app, &self.logger, store_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_view_script_carries_the_close_hook_only_for_popups_running_it() {
        assert_eq!(service_view_script("s", None, false), "s");
        assert_eq!(service_view_script("s", Some("x"), false), "s\nx");
        assert_eq!(
            service_view_script("s", Some("x"), true),
            format!("s\nx\n{CLOSE_HOOK}")
        );
        assert_eq!(
            service_view_script("s", None, true),
            format!("s\n{CLOSE_HOOK}")
        );
    }
}
