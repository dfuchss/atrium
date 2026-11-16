//! Service views: one child webview per account in the main window, with its native hooks.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Duration;

use domain::ports::{PortError, PortResult, WebviewHost};
use domain::{Account, Bounds, ViewPolicy, ViewSpec, redact_url, unused_store_ids};
use protocol::{Snapshot, UI_WEBVIEW};
use tauri::utils::config::BackgroundThrottlingPolicy;
use tauri::webview::{DownloadEvent, PageLoadEvent};
use tauri::{AppHandle, Manager, Runtime, Url, Webview, WebviewBuilder, WebviewUrl, Window};
use uuid::Uuid;

use crate::dialogs::Dialogs;
use crate::links::{CLOSE_HOOK, LinkPolicy, navigation_hook, permission_hook, popup_label_prefix};
use crate::logger::spawn_logged;
use crate::native;
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

/// Longest wait for the views and windows of a removed account to disappear before its data
/// store is deleted anyway (the retries below may still cover a slow close).
const ACCOUNT_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
const ACCOUNT_CLOSE_POLL: Duration = Duration::from_millis(100);

/// True if `label` is the service view or an in-app window of `account`.
fn belongs_to_account(label: &str, account: Uuid) -> bool {
    label == view_label(account) || label.starts_with(&popup_label_prefix(account))
}

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

/// Blocks until no view or window of `account` remains, for at most [`ACCOUNT_CLOSE_TIMEOUT`]:
/// `WebKit` refuses to delete a data store that a live page still uses.
fn wait_until_account_closed<R: Runtime>(app: &AppHandle<R>, account: Uuid, logger: &Logger) {
    let mut waited = Duration::ZERO;
    while app
        .webviews()
        .keys()
        .any(|label| belongs_to_account(label, account))
    {
        if waited >= ACCOUNT_CLOSE_TIMEOUT {
            logger.log(&format!(
                "[{account}] views or windows still open after {} s, deleting data anyway",
                waited.as_secs()
            ));
            return;
        }
        std::thread::sleep(ACCOUNT_CLOSE_POLL);
        waited += ACCOUNT_CLOSE_POLL;
    }
}

/// Deletes the data store `store_id`, retrying after [`REMOVE_STORE_RETRY_DELAYS`]. Blocks, so
/// it runs on a thread of its own, never on the actor or the main thread.
fn remove_data_store_with_retries<R: Runtime>(
    app: &AppHandle<R>,
    store_id: [u8; 16],
    logger: &Logger,
) {
    let store = Uuid::from_bytes(store_id);
    let mut result = native::remove_data_store(app, store_id);
    for delay in REMOVE_STORE_RETRY_DELAYS {
        let Err(error) = &result else {
            break;
        };
        logger.log(&format!(
            "cannot delete data store {store}: {error}; retrying in {} s",
            delay.as_secs()
        ));
        std::thread::sleep(delay);
        result = native::remove_data_store(app, store_id);
    }
    match result {
        Ok(()) => logger.log(&format!("deleted data store {store}")),
        Err(error) => logger.log(&format!(
            "giving up deleting data store {store}: {error} (removed again at the next start)"
        )),
    }
}

/// Start of the labels of service webviews.
const VIEW_LABEL_PREFIX: &str = "svc-";

/// Label of the service webview of `account`.
pub(crate) fn view_label(account: Uuid) -> String {
    format!("{VIEW_LABEL_PREFIX}{}", account.simple())
}

/// The account whose service webview has `label`; None for any other webview.
pub(crate) fn view_account(label: &str) -> Option<Uuid> {
    let id = label.strip_prefix(VIEW_LABEL_PREFIX)?;
    Uuid::try_parse(id).ok()
}

/// `dir/name`, or `dir/stem (n).ext` for the first `n` that does not exist yet, so a download
/// never replaces an existing file.
fn unused_path(dir: &Path, name: &OsStr) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let original = Path::new(name);
    let stem = original.file_stem().unwrap_or(name).to_string_lossy();
    let extension = original.extension().map_or_else(String::new, |extension| {
        format!(".{}", extension.to_string_lossy())
    });
    (1..u32::MAX)
        .map(|n| dir.join(format!("{stem} ({n}){extension}")))
        .find(|path| !path.exists())
        .unwrap_or(candidate)
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
                *destination = unused_path(&dir, OsStr::new(&name));
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
        Dialogs::attach(&webview, &self.logger);
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
        native::force_reload(&self.webview(account)?)
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
        // A data store id is its account's id.
        let account = Uuid::from_bytes(store_id);
        spawn_logged("remove-store", &self.logger, move || {
            wait_until_account_closed(&app, account, &logger);
            remove_data_store_with_retries(&app, store_id, &logger);
        });
        Ok(())
    }
}

/// Whether the data store of `account` is in use now: the account is one of `current` (the
/// accounts of the live snapshot) or one of its views or windows is open (`labels`). The sweep lists the stores while the
/// app already runs, so an account added meanwhile has a store that the startup list lacks; its
/// view is open before the snapshot shows the account.
fn store_in_use<'a>(
    account: Uuid,
    current: &[Uuid],
    mut labels: impl Iterator<Item = &'a str>,
) -> bool {
    current.contains(&account) || labels.any(|label| belongs_to_account(label, account))
}

/// The ids of the accounts in `snapshot`.
fn account_ids(snapshot: &Mutex<Snapshot>) -> Vec<Uuid> {
    let snapshot = snapshot.lock().unwrap_or_else(PoisonError::into_inner);
    snapshot.accounts.iter().map(|view| view.id).collect()
}

impl<R: Runtime> TauriHost<R> {
    /// Deletes, in the background, the data stores of this app that belong to none of
    /// `accounts` and are not in use when they are deleted (see [`store_in_use`]; `current` is
    /// the live snapshot). Call it only with the complete account list (see
    /// `FsStore::lists_all_accounts`).
    pub fn remove_unused_data_stores(&self, accounts: Vec<Account>, current: Arc<Mutex<Snapshot>>) {
        let (app, logger) = (self.app.clone(), self.logger.clone());
        spawn_logged("unused-stores", &self.logger, move || {
            let existing = match native::data_store_ids(&app) {
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
                let account = Uuid::from_bytes(store_id);
                let in_use = store_in_use(
                    account,
                    &account_ids(&current),
                    app.webviews().keys().map(String::as_str),
                );
                if in_use {
                    logger.log(&format!(
                        "keeping data store {account}: its account was added"
                    ));
                } else {
                    remove_data_store_with_retries(&app, store_id, &logger);
                }
            }
        });
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

    #[test]
    fn stores_of_current_accounts_or_open_views_are_in_use() {
        let (listed, viewed, orphan) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let current = [listed];
        let labels = [view_label(viewed), UI_WEBVIEW.to_string()];
        let open = || labels.iter().map(String::as_str);
        assert!(store_in_use(listed, &current, open()));
        assert!(store_in_use(viewed, &current, open()));
        assert!(!store_in_use(orphan, &current, open()));
        let popup = [crate::links::popup_label(orphan, 4)];
        assert!(store_in_use(
            orphan,
            &current,
            popup.iter().map(String::as_str)
        ));
    }

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
    fn account_labels_do_not_match_other_accounts() {
        let (first, second) = (Uuid::from_u128(1), Uuid::from_u128(2));
        assert!(belongs_to_account(&view_label(first), first));
        assert!(belongs_to_account(
            &crate::links::popup_label(first, 7),
            first
        ));
        assert!(!belongs_to_account(&view_label(second), first));
        assert!(!belongs_to_account(
            &crate::links::popup_label(second, 7),
            first
        ));
        assert!(!belongs_to_account("ui", first));
    }

    #[test]
    fn view_account_reads_service_view_labels_only() {
        let account = Uuid::from_u128(0x2a);
        assert_eq!(view_account(&view_label(account)), Some(account));
        assert_eq!(view_account(UI_WEBVIEW), None);
        assert_eq!(view_account(&crate::links::popup_label(account, 1)), None);
        assert_eq!(view_account("svc-nonsense"), None);
    }

    #[test]
    fn view_label_uses_simple_uuid() {
        let id = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
        assert_eq!(view_label(id), "svc-0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn downloads_never_replace_existing_files() {
        let dir = std::env::temp_dir().join(format!("atrium-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let name = OsStr::new("report.pdf");
        assert_eq!(unused_path(&dir, name), dir.join("report.pdf"));
        std::fs::write(dir.join("report.pdf"), "x").unwrap();
        assert_eq!(unused_path(&dir, name), dir.join("report (1).pdf"));
        std::fs::write(dir.join("report (1).pdf"), "x").unwrap();
        assert_eq!(unused_path(&dir, name), dir.join("report (2).pdf"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
