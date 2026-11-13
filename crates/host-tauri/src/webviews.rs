//! Service views: one child webview per account in the main window, with its native hooks.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use domain::ports::{PortError, PortResult, WebviewHost};
use domain::{Account, Bounds, ViewPolicy, ViewSpec, redact_url, unused_store_ids};
use protocol::UI_WEBVIEW;
use tauri::utils::config::BackgroundThrottlingPolicy;
use tauri::webview::{DownloadEvent, PageLoadEvent};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, Url, Webview, WebviewBuilder,
    WebviewUrl,
};
use uuid::Uuid;

use crate::links::{LinkPolicy, navigation_hook, permission_hook};
use crate::{Feed, Input, Logger, TauriHost, port_err};

/// Time `WebKit` gets to delete the browsing data before the view reloads.
const CLEAR_DATA_RELOAD_DELAY: Duration = Duration::from_millis(800);

/// Pauses before retrying a failed data store deletion: right after its view closed, `WebKit`
/// may still hold the store.
const REMOVE_STORE_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(30),
];

/// Deletes the data store `store_id`, retrying after [`REMOVE_STORE_RETRY_DELAYS`]. Blocks, so
/// it runs on a thread of its own, never on the actor or the main thread.
fn remove_data_store_with_retries<R: Runtime>(
    app: &AppHandle<R>,
    store_id: [u8; 16],
    logger: &Logger,
) {
    let store = Uuid::from_bytes(store_id);
    let mut result = tauri::async_runtime::block_on(app.remove_data_store(store_id));
    for delay in REMOVE_STORE_RETRY_DELAYS {
        let Err(error) = &result else {
            break;
        };
        logger.log(&format!(
            "cannot delete data store {store}: {error}; retrying in {} s",
            delay.as_secs()
        ));
        std::thread::sleep(delay);
        result = tauri::async_runtime::block_on(app.remove_data_store(store_id));
    }
    match result {
        Ok(()) => logger.log(&format!("deleted data store {store}")),
        Err(error) => logger.log(&format!(
            "giving up deleting data store {store}: {error} (removed again at the next start)"
        )),
    }
}

/// Runs `job` on a new thread named `name`; logs if the thread cannot start.
fn spawn_logged(name: &str, logger: &Logger, job: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(job) {
        logger.log(&format!("cannot start thread {name}: {error}"));
    }
}

/// Label of the service webview of `account`.
pub(crate) fn view_label(account: Uuid) -> String {
    format!("svc-{}", account.simple())
}

fn place<R: Runtime>(webview: &Webview<R>, bounds: Bounds) -> PortResult {
    webview
        .set_position(LogicalPosition::new(bounds.x, bounds.y))
        .map_err(port_err)?;
    webview
        .set_size(LogicalSize::new(bounds.width, bounds.height))
        .map_err(port_err)
}

/// Name a download is saved under: the file name the page suggested, or None for hidden
/// (dot) files, which would land unseen in Downloads (e.g. `.zshrc`, `.htaccess`).
fn download_file_name(suggested: &Path) -> Option<String> {
    let name = suggested.file_name().map_or_else(
        || "download".into(),
        |name| name.to_string_lossy().into_owned(),
    );
    (!name.starts_with('.')).then_some(name)
}

/// Download handler: saves into the user's Downloads folder (quarantined by macOS, see
/// `LSFileQuarantineEnabled` in Info.plist), refuses hidden files and reports finished
/// downloads as [`Input::DownloadFinished`]. On macOS `Finished` carries no path, so the name
/// is remembered per URL from `Requested`.
fn download_hook<R: Runtime>(
    app: AppHandle<R>,
    account: Uuid,
    logger: Logger,
    feed: Feed,
) -> impl Fn(Webview<R>, DownloadEvent<'_>) -> bool + Send + Sync + 'static {
    let pending: Mutex<HashMap<Url, String>> = Mutex::default();
    move |_, event| match event {
        DownloadEvent::Requested { url, destination } => {
            let shown_url = redact_url(url.as_str());
            let Some(name) = download_file_name(destination) else {
                logger.log(&format!(
                    "[{account}] download {shown_url} refused: hidden file name"
                ));
                return false;
            };
            if let Ok(dir) = app.path().download_dir() {
                *destination = dir.join(&name);
            }
            logger.log(&format!(
                "[{account}] download {shown_url} -> {}",
                destination.display()
            ));
            pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(url, name);
            true
        }
        DownloadEvent::Finished { url, success, .. } => {
            logger.log(&format!(
                "[{account}] download finished {} ok={success}",
                redact_url(url.as_str())
            ));
            let name = pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&url);
            if let (true, Some(file_name)) = (success, name) {
                feed(Input::DownloadFinished { account, file_name });
            }
            true
        }
        _ => true,
    }
}

impl<R: Runtime> TauriHost<R> {
    fn webview(&self, account: Uuid) -> PortResult<Webview<R>> {
        self.app
            .get_webview(&view_label(account))
            .ok_or_else(|| PortError(format!("no view for account {account}")))
    }

    /// The view's init script plus the development script, if any.
    fn init_script(&self, spec: &ViewSpec) -> String {
        match &self.extra_init_script {
            Some(extra) => format!("{}\n{extra}", spec.init_script),
            None => spec.init_script.clone(),
        }
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
            feed: self.feed.clone(),
            logger: self.logger.clone(),
        });
        let (feed_title, feed_load) = (self.feed.clone(), self.feed.clone());
        let load_logger = self.logger.clone();
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
        let mut builder = WebviewBuilder::new(label, WebviewUrl::External(spec.url.clone()))
            .data_store_identifier(spec.store_id)
            .initialization_script(self.init_script(spec))
            // Hidden accounts keep running (calls, sockets, timers).
            .background_throttling(BackgroundThrottlingPolicy::Disabled);
        if let Some(user_agent) = &spec.user_agent {
            builder = builder.user_agent(user_agent);
        }
        let builder = self.hook(spec, builder);
        // Safe off the main thread: add_child builds on the main thread and waits for it.
        let webview = self
            .window()?
            .add_child(
                builder,
                LogicalPosition::new(bounds.x, bounds.y),
                LogicalSize::new(bounds.width, bounds.height),
            )
            .map_err(port_err)?;
        if !visible {
            webview.hide().map_err(port_err)?;
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
        place(&self.webview(account)?, bounds)
    }

    fn set_ui_bounds(&self, bounds: Bounds) -> PortResult {
        let ui = self
            .app
            .get_webview(UI_WEBVIEW)
            .ok_or_else(|| PortError(format!("webview {UI_WEBVIEW:?} not found")))?;
        place(&ui, bounds)
    }

    fn show(&self, account: Uuid) -> PortResult {
        self.webview(account)?.show().map_err(port_err)
    }

    fn hide(&self, account: Uuid) -> PortResult {
        self.webview(account)?.hide().map_err(port_err)
    }

    fn destroy(&self, account: Uuid) -> PortResult {
        self.webview(account)?.close().map_err(port_err)
    }

    fn reload(&self, account: Uuid) -> PortResult {
        self.webview(account)?.reload().map_err(port_err)
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
        std::thread::spawn(move || {
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
        let (app, logger) = (self.app.clone(), self.logger.clone());
        spawn_logged("remove-store", &self.logger, move || {
            remove_data_store_with_retries(&app, store_id, &logger);
        });
        Ok(())
    }
}

impl<R: Runtime> TauriHost<R> {
    /// Deletes, in the background, the data stores of this app that belong to none of
    /// `accounts`. Call it only with the complete account list (see `FsStore::lists_all_accounts`).
    pub fn remove_unused_data_stores(&self, accounts: Vec<Account>) {
        let (app, logger) = (self.app.clone(), self.logger.clone());
        spawn_logged("unused-stores", &self.logger, move || {
            let existing = match tauri::async_runtime::block_on(app.fetch_data_store_identifiers())
            {
                Ok(existing) => existing,
                Err(error) => {
                    logger.log(&format!("cannot list data stores: {error}"));
                    return;
                }
            };
            let unused = unused_store_ids(&existing, &accounts);
            logger.log(&format!(
                "{} data stores, {} without account",
                existing.len(),
                unused.len()
            ));
            for store_id in unused {
                remove_data_store_with_retries(&app, store_id, &logger);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_downloads_are_refused() {
        assert_eq!(
            download_file_name(Path::new("/tmp/report.pdf")).as_deref(),
            Some("report.pdf")
        );
        assert_eq!(
            download_file_name(Path::new("/")).as_deref(),
            Some("download")
        );
        assert_eq!(download_file_name(Path::new("/tmp/.zshrc")), None);
        assert_eq!(download_file_name(Path::new(".hidden")), None);
    }

    #[test]
    fn view_label_uses_simple_uuid() {
        let id = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
        assert_eq!(view_label(id), "svc-0123456789abcdef0123456789abcdef");
    }
}
