//! Tauri 2 adapter: implements the domain ports ([`WebviewHost`], [`Notifier`], [`Platform`],
//! [`UiSink`]) on top of an [`AppHandle`].
//!
//! Asynchronous results (title changes, page loads, poll callbacks, external links,
//! notification clicks) are turned into [`Input`]s and handed to `feed`, which must not block
//! (the app sends them on the actor's channel).
//!
//! Threading: every method may be called from any thread. Tauri marshals webview and window
//! calls onto the main thread itself (`Window::add_child` runs the build via
//! `run_on_main_thread` and waits for it; setters/`eval` post a message to the event loop).
//! Blocking calls (`add_child`, menu and tray building) wait for the main thread, so the main
//! thread must never wait on the caller.
//!
//! New-window policy: see [`links`]. Native menus and tray: see [`menus`].

mod icons;
pub mod links;
pub mod menus;

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use domain::ports::{Notifier, Platform, PortError, PortResult, UiSink, WebviewHost};
use domain::{Bounds, DockBadge, Input, MenuModel, NotificationReq, ViewPolicy, ViewSpec};
use protocol::{AppSettings, EVENT_SNAPSHOT, Snapshot, UI_WEBVIEW};
use tauri::utils::config::BackgroundThrottlingPolicy;
use tauri::webview::{DownloadEvent, PageLoadEvent};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Runtime, Webview, WebviewBuilder,
    WebviewUrl, Window,
};
use tauri_plugin_autostart::AutoLaunchManager;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

use crate::links::{LinkPolicy, ViewPolicies, navigation_hook, permission_hook};
use crate::menus::{MenuState, menu_command};

/// Sink for inputs produced by the adapter; must not block.
pub type Feed = Arc<dyn Fn(Input) + Send + Sync>;

/// Sound name that makes `NSUserNotification` play the user's default notification sound
/// (mac-notification-sys maps exactly this string to `NSUserNotificationDefaultSoundName`;
/// other strings are looked up as named system sounds, unknown names play nothing).
const DEFAULT_SOUND: &str = "NSUserNotificationDefaultSoundName";

/// Native notifications remembered for click routing; older ones are forgotten.
const MAX_TRACKED_NOTIFICATIONS: usize = 512;

/// Label of the service webview of `account`.
pub fn view_label(account: Uuid) -> String {
    format!("svc-{}", account.simple())
}

#[derive(Default)]
struct Notifications {
    next_id: i32,
    /// native notification id -> (account, shim id)
    targets: HashMap<i32, (Uuid, u64)>,
}

/// Timestamped logging to stdout and `<app_log_dir>/messengers.log`.
#[derive(Clone)]
pub(crate) struct Logger {
    file: Option<Arc<PathBuf>>,
}

impl Logger {
    fn new<R: Runtime>(app: &AppHandle<R>) -> Logger {
        let file = app.path().app_log_dir().ok().and_then(|dir| {
            std::fs::create_dir_all(&dir).ok()?;
            Some(Arc::new(dir.join("messengers.log")))
        });
        Logger { file }
    }

    fn log(&self, message: &str) {
        let line = format!("{} {message}", timestamp());
        println!("{line}");
        if let Some(path) = &self.file
            && let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path.as_path())
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// `YYYY-MM-DD HH:MM:SS.mmmZ` (UTC) without a date crate.
fn timestamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        now.subsec_millis()
    )
}

fn port_err(e: impl std::fmt::Display) -> PortError {
    PortError(e.to_string())
}

/// Implements all webview-engine and OS ports with Tauri.
pub struct TauriHost<R: Runtime> {
    app: AppHandle<R>,
    window_label: String,
    feed: Feed,
    notifications: Arc<Mutex<Notifications>>,
    logger: Logger,
    policies: ViewPolicies,
    /// Numbers the labels of popup/app windows.
    popup_counter: Arc<AtomicU64>,
    menus: Mutex<MenuState<R>>,
}

impl<R: Runtime> TauriHost<R> {
    /// `window_label` names the window hosting the `ui` and service webviews. Also installs the
    /// notification click handler (the notification plugin must be registered).
    pub fn new(app: AppHandle<R>, window_label: impl Into<String>, feed: Feed) -> TauriHost<R> {
        let host = TauriHost {
            logger: Logger::new(&app),
            app,
            window_label: window_label.into(),
            feed,
            notifications: Arc::new(Mutex::new(Notifications::default())),
            policies: Arc::default(),
            popup_counter: Arc::default(),
            menus: Mutex::default(),
        };
        host.register_notification_clicks();
        host
    }

    /// File the host logger appends to (`<app_log_dir>/messengers.log`), if available.
    pub fn log_file(&self) -> Option<PathBuf> {
        self.logger.file.as_deref().cloned()
    }

    /// Routes clicks on native notifications back as [`Input::NotificationClicked`].
    /// Called by [`TauriHost::new`]; notifications shown before registration report nothing.
    fn register_notification_clicks(&self) {
        let (feed, notifications) = (self.feed.clone(), self.notifications.clone());
        let result = self.app.notification().on_action(move |action| {
            let Some(native_id) = action.notification().map(|n| n.id()) else { return };
            let target = notifications
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .targets
                .get(&native_id)
                .copied();
            if let Some((account, shim_id)) = target {
                feed(Input::NotificationClicked { account, shim_id });
            }
        });
        if let Err(e) = result {
            self.logger.log(&format!("cannot register notification click handler: {e}"));
        }
    }

    fn window(&self) -> PortResult<Window<R>> {
        self.app
            .get_window(&self.window_label)
            .ok_or_else(|| PortError(format!("window {:?} not found", self.window_label)))
    }

    fn webview(&self, account: Uuid) -> PortResult<Webview<R>> {
        self.app
            .get_webview(&view_label(account))
            .ok_or_else(|| PortError(format!("no view for account {account}")))
    }

    fn place(webview: &Webview<R>, bounds: Bounds) -> PortResult {
        webview.set_position(LogicalPosition::new(bounds.x, bounds.y)).map_err(port_err)?;
        webview.set_size(LogicalSize::new(bounds.width, bounds.height)).map_err(port_err)
    }

    /// Adds the native hooks of one service view to `builder`.
    fn hook(&self, spec: &ViewSpec, builder: WebviewBuilder<R>) -> WebviewBuilder<R> {
        let account = spec.account;
        let policy = Arc::new(LinkPolicy {
            app: self.app.clone(),
            spec: Arc::new(spec.clone()),
            policies: self.policies.clone(),
            counter: self.popup_counter.clone(),
            feed: self.feed.clone(),
            logger: self.logger.clone(),
        });
        let (feed_title, feed_load) = (self.feed.clone(), self.feed.clone());
        let log_load = self.logger.clone();
        let log_download = self.logger.clone();
        let app = self.app.clone();
        builder
            .on_document_title_changed(move |_, title| {
                feed_title(Input::TitleChanged { account, title });
            })
            .on_page_load(move |_, payload| {
                if let PageLoadEvent::Finished = payload.event() {
                    log_load.log(&format!("[{account}] loaded {}", payload.url()));
                    feed_load(Input::PageLoaded { account, url: payload.url().to_string() });
                }
            })
            // Fires for iframe loads too (no frame info), so it must never open anything
            // externally (sign-in and captcha iframes would end up in the browser); it only
            // allows or denies by scheme (`domain::nav_allowed`).
            .on_navigation(navigation_hook(account, self.logger.clone(), self.feed.clone()))
            // Never Allow: see `links` (sign-in popups -> in-app window, else the link target).
            .on_new_window(move |url, features| policy.on_new_window(url, features))
            .on_download(move |_, event| {
                match event {
                    DownloadEvent::Requested { url, destination } => {
                        let name = destination
                            .file_name()
                            .map(ToOwned::to_owned)
                            .unwrap_or_else(|| "download".into());
                        if let Ok(dir) = app.path().download_dir() {
                            *destination = dir.join(name);
                        }
                        log_download.log(&format!("[{account}] download {url} -> {}", destination.display()));
                    }
                    DownloadEvent::Finished { url, path, success } => {
                        log_download.log(&format!("[{account}] download finished {url} {path:?} ok={success}"));
                    }
                    _ => {}
                }
                true
            })
            .on_permission_request(permission_hook(account, self.policies.clone(), self.logger.clone(), "view"))
    }
}

impl<R: Runtime> WebviewHost for TauriHost<R> {
    fn create(&self, spec: &ViewSpec, bounds: Bounds, visible: bool) -> PortResult {
        let label = view_label(spec.account);
        if self.app.get_webview(&label).is_some() {
            return Err(PortError(format!("view for account {} already exists", spec.account)));
        }
        let mut builder = WebviewBuilder::new(label, WebviewUrl::External(spec.url.clone()))
            .data_store_identifier(spec.store_id)
            .initialization_script(&spec.init_script)
            // Hidden accounts keep running (calls, sockets, timers).
            .background_throttling(BackgroundThrottlingPolicy::Disabled);
        if let Some(ua) = &spec.user_agent {
            builder = builder.user_agent(ua);
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
        self.logger.log(&format!("[{}] created view {} ({})", spec.account, webview.label(), spec.url));
        Ok(())
    }

    fn set_bounds(&self, account: Uuid, bounds: Bounds) -> PortResult {
        Self::place(&self.webview(account)?, bounds)
    }

    fn set_ui_bounds(&self, bounds: Bounds) -> PortResult {
        let ui = self
            .app
            .get_webview(UI_WEBVIEW)
            .ok_or_else(|| PortError(format!("webview {UI_WEBVIEW:?} not found")))?;
        Self::place(&ui, bounds)
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
        self.policies.lock().unwrap_or_else(PoisonError::into_inner).insert(account, policy);
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
            std::thread::sleep(Duration::from_millis(800));
            let result = webview.reload();
            logger.log(&format!("[{account}] cleared browsing data, reload -> {result:?}"));
        });
        Ok(())
    }

    fn current_url(&self, account: Uuid) -> PortResult<String> {
        self.webview(account)?.url().map(|u| u.to_string()).map_err(port_err)
    }

    fn find_icon(&self, account: Uuid, script: &str) -> PortResult {
        let feed = self.feed.clone();
        self.webview(account)?
            .eval_with_callback(script, move |raw| feed(Input::IconFound { account, raw }))
            .map_err(port_err)
    }

    fn delete_profile(&self, store_id: [u8; 16]) -> PortResult {
        let (app, logger) = (self.app.clone(), self.logger.clone());
        tauri::async_runtime::spawn(async move {
            match app.remove_data_store(store_id).await {
                Ok(()) => logger.log(&format!("deleted data store {}", Uuid::from_bytes(store_id))),
                Err(e) => logger.log(&format!("cannot delete data store {}: {e}", Uuid::from_bytes(store_id))),
            }
        });
        Ok(())
    }
}

impl<R: Runtime> Notifier for TauriHost<R> {
    fn notify(&self, req: &NotificationReq) -> PortResult {
        let native_id = {
            let mut n = self.notifications.lock().unwrap_or_else(PoisonError::into_inner);
            n.next_id = n.next_id.checked_add(1).unwrap_or(1);
            let id = n.next_id;
            n.targets.insert(id, (req.account, req.shim_id));
            if n.targets.len() > MAX_TRACKED_NOTIFICATIONS {
                let oldest = id.saturating_sub(MAX_TRACKED_NOTIFICATIONS as i32);
                n.targets.retain(|&k, _| k > oldest);
            }
            id
        };
        let mut builder = self.app.notification().builder().id(native_id).title(&req.title).body(&req.body);
        if req.sound {
            builder = builder.sound(DEFAULT_SOUND);
        }
        builder.show().map_err(port_err)
    }
}

impl<R: Runtime> Platform for TauriHost<R> {
    fn set_dock_badge(&self, badge: DockBadge) -> PortResult {
        let window = self.window()?;
        match badge {
            DockBadge::Count(n) if n > 0 => window.set_badge_count(Some(i64::from(n))),
            DockBadge::Dot => window.set_badge_label(Some("•".to_string())),
            DockBadge::None | DockBadge::Count(_) => window.set_badge_label(None),
        }
        .map_err(port_err)
    }

    fn open_external(&self, url: &str, browser: Option<&str>) -> PortResult {
        // With a browser this runs `/usr/bin/open -a <browser> -- <url>` and waits for it.
        let result = self.app.opener().open_url(url, browser).map_err(port_err);
        self.logger.log(&format!("open external {url} (browser {browser:?}) -> {result:?}"));
        result
    }

    fn apply_system_settings(&self, settings: &AppSettings) -> PortResult {
        let autostart = self
            .app
            .try_state::<AutoLaunchManager>()
            .ok_or_else(|| PortError("autostart plugin not registered".into()))?;
        let enabled = autostart.is_enabled().map_err(port_err)?;
        if enabled != settings.launch_at_login {
            if settings.launch_at_login {
                autostart.enable().map_err(port_err)?;
            } else {
                autostart.disable().map_err(port_err)?;
            }
            self.logger.log(&format!("launch at login: {enabled} -> {}", settings.launch_at_login));
        }
        Ok(())
    }

    fn focus_window(&self) -> PortResult {
        let window = self.window()?;
        window.unminimize().map_err(port_err)?;
        window.show().map_err(port_err)?;
        window.set_focus().map_err(port_err)
    }

    fn fetch_icon(&self, account: Uuid, url: &str) -> PortResult {
        let (feed, logger, url) = (self.feed.clone(), self.logger.clone(), url.to_string());
        std::thread::Builder::new()
            .name("icon".into())
            .spawn(move || {
                let data_url = match icons::download_icon(&url) {
                    Ok(d) => {
                        logger.log(&format!("[{account}] icon {url} ({} bytes as data URL)", d.len()));
                        Some(d)
                    }
                    Err(e) => {
                        logger.log(&format!("[{account}] icon {url} failed: {e}"));
                        None
                    }
                };
                feed(Input::IconFetched { account, data_url });
            })
            .map(drop)
            .map_err(port_err)
    }

    fn set_menus(&self, model: &MenuModel) -> PortResult {
        self.menus.lock().unwrap_or_else(PoisonError::into_inner).apply(&self.app, model).map_err(port_err)
    }

    fn log(&self, message: &str) {
        self.logger.log(message);
    }
}

/// Handles a click on an item of the app menu or the tray menu (main thread): shows the
/// window `window_label` if the item asks for it, then feeds the item's input.
pub fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, window_label: &str, id: &str, feed: &Feed) {
    let Some(cmd) = menu_command(id) else { return };
    if cmd.show_window
        && let Some(window) = app.get_window(window_label)
    {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if let Some(input) = cmd.input {
        feed(input);
    }
}

impl<R: Runtime> UiSink for TauriHost<R> {
    fn emit_snapshot(&self, snapshot: &Snapshot) -> PortResult {
        self.app.emit_to(UI_WEBVIEW, EVENT_SNAPSHOT, snapshot).map_err(port_err)
    }
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
    fn timestamp_has_fixed_shape() {
        let ts = timestamp();
        assert_eq!(ts.len(), "2026-11-13 12:00:00.000Z".len());
        assert!(ts.starts_with("20"));
    }
}
