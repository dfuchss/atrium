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
//! New-window policy: see `links`. JavaScript dialogs: see `dialogs`. Native menus and tray:
//! see `menus`. Quitting: see [`graceful_quit`]. What is specific to macOS: see `native`.
//!
//! [`WebviewHost`]: domain::ports::WebviewHost
//! [`Notifier`]: domain::ports::Notifier
//! [`Platform`]: domain::ports::Platform

mod dialogs;
mod icons;
mod links;
mod logger;
mod menus;
mod native;
mod notifications;
mod platform;
mod quit;
pub mod services;
mod webviews;

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, PoisonError};

use domain::ports::{PortError, PortResult, UiSink};
use domain::{Bounds, Input};
use protocol::{EVENT_SNAPSHOT, Snapshot, UI_WEBVIEW};
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager, Runtime, Theme, WebviewBuilder, WebviewUrl, Window};

use crate::dialogs::{DialogPolicy, SharedDialogPolicy, TauriDialogHandler, account_names};
use crate::links::{PopupQuota, ViewPolicies, new_popup_quota};
use crate::logger::Logger;
use crate::menus::MenuState;
use crate::notifications::NotificationTargets;

/// Sidebar background (`--bg` in `app/ui/styles.css`), shown through the transparent title bar.
const SIDEBAR_BG_DARK: Color = Color(0x1b, 0x1c, 0x1f, 0xff);
const SIDEBAR_BG_LIGHT: Color = Color(0xf1, 0xf2, 0xf4, 0xff);

/// Window background for `theme`: the transparent title bar shows it, so it matches the sidebar.
#[must_use]
pub fn window_background(theme: Theme) -> Color {
    if theme == Theme::Dark {
        SIDEBAR_BG_DARK
    } else {
        SIDEBAR_BG_LIGHT
    }
}

/// The native appearance for a theme setting; `None` follows the system.
#[must_use]
pub fn native_theme(theme: protocol::Theme) -> Option<Theme> {
    match theme {
        protocol::Theme::Auto => None,
        protocol::Theme::Light => Some(Theme::Light),
        protocol::Theme::Dark => Some(Theme::Dark),
    }
}

pub use crate::menus::handle_menu_event;
pub use crate::quit::{graceful_quit, is_quitting};

/// Sink for inputs produced by the adapter; must not block.
pub type Feed = Arc<dyn Fn(Input) + Send + Sync>;

fn port_err(error: impl std::fmt::Display) -> PortError {
    PortError(error.to_string())
}

/// Unminimizes, shows and focuses `window`; tries every step and reports the first error.
pub fn bring_to_front<R: Runtime>(window: &Window<R>) -> tauri::Result<()> {
    let unminimized = window.unminimize();
    let shown = window.show();
    let focused = window.set_focus();
    unminimized.and(shown).and(focused)
}

/// Adds the sidebar (`ui`) webview showing `url` at the left edge of `window`.
pub fn add_sidebar<R: Runtime>(
    window: &Window<R>,
    url: WebviewUrl,
    width: f64,
    height: f64,
) -> tauri::Result<()> {
    let bounds = Bounds {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    native::add_child(window, WebviewBuilder::new(UI_WEBVIEW, url), bounds)?;
    Ok(())
}

/// Shows a modal warning with an OK button on the main thread (inline when called there),
/// e.g. before the main window exists.
pub fn show_warning<R: Runtime>(app: &AppHandle<R>, title: &str, text: &str) -> tauri::Result<()> {
    native::show_warning(app, title, text)
}

/// Calls `on_wake` each time the displays wake, until the app exits.
pub fn observe_screens_wake(on_wake: impl Fn() + Send + Sync + 'static) {
    native::observe_screens_wake(on_wake);
}

/// Has the core reload a service view whose web content process ended; other webviews are
/// reloaded in place.
#[cfg(target_os = "macos")]
pub fn register_crash_hook<R: Runtime>(
    builder: tauri::Builder<R>,
    feed: &Feed,
) -> tauri::Builder<R> {
    native::register_crash_hook(builder, feed)
}

/// Implements all webview-engine and OS ports with Tauri.
pub struct TauriHost<R: Runtime> {
    app: AppHandle<R>,
    window_label: String,
    feed: Feed,
    notifications: Arc<Mutex<NotificationTargets>>,
    logger: Logger,
    policies: ViewPolicies,
    dialogs: SharedDialogPolicy,
    /// Numbers the labels of popup/app windows.
    popup_counter: Arc<AtomicU64>,
    /// Limits popup/app windows per account.
    popup_quota: PopupQuota,
    menus: Mutex<MenuState<R>>,
    /// Development override of the config folder (see [`TauriHost::with_config_dir`]).
    config_dir: Option<PathBuf>,
    /// Development aid appended to every service view's init script.
    extra_init_script: Option<String>,
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
            notifications: Arc::default(),
            policies: Arc::default(),
            dialogs: Arc::new(Mutex::new(DialogPolicy::new())),
            popup_counter: Arc::default(),
            popup_quota: new_popup_quota(),
            menus: Mutex::default(),
            config_dir: None,
            extra_init_script: None,
        };
        host.register_notification_clicks();
        host
    }

    /// Development: use `dir` instead of the app config folder for the services folder and the
    /// log file (`<dir>/atrium.log`), and never change the login item, so a second instance
    /// leaves the regular installation alone.
    #[must_use]
    pub fn with_config_dir(mut self, dir: PathBuf) -> Self {
        self.logger = Logger::in_dir(&dir);
        self.config_dir = Some(dir);
        self
    }

    /// Development: JavaScript appended to the init script of every service view.
    #[must_use]
    pub fn with_extra_init_script(mut self, script: String) -> Self {
        self.extra_init_script = Some(script);
        self
    }

    /// Makes `alert`, `confirm` and `prompt` of the service views show native panels. Call it
    /// once the sidebar webview exists and before the first service view is created (see
    /// `native::install_dialogs`); without it the pages get the default answers.
    pub fn install_dialogs(&self) {
        let handler = Arc::new(TauriDialogHandler {
            policy: self.dialogs.clone(),
            logger: self.logger.clone(),
        });
        native::install_dialogs(&self.app, handler, &self.logger);
    }

    /// File the host logger appends to (`<app_log_dir>/atrium.log`), if available.
    pub fn log_file(&self) -> Option<PathBuf> {
        self.logger.file()
    }

    /// The host log as a plain function, for parts outside the ports (the updater).
    pub fn log_sink(&self) -> Arc<dyn Fn(&str) + Send + Sync> {
        let logger = self.logger.clone();
        Arc::new(move |message| logger.log(message))
    }

    /// `<config>/services`, the user's service definition files.
    fn services_dir(&self) -> PortResult<PathBuf> {
        let config_dir = match &self.config_dir {
            Some(dir) => dir.clone(),
            None => self.app.path().app_config_dir().map_err(port_err)?,
        };
        Ok(config_dir.join("services"))
    }

    fn window(&self) -> PortResult<Window<R>> {
        self.app
            .get_window(&self.window_label)
            .ok_or_else(|| PortError(format!("window {:?} not found", self.window_label)))
    }
}

impl<R: Runtime> UiSink for TauriHost<R> {
    fn emit_snapshot(&self, snapshot: &Snapshot) -> PortResult {
        self.dialogs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .set_names(account_names(snapshot));
        self.app
            .emit_to(UI_WEBVIEW, EVENT_SNAPSHOT, snapshot)
            .map_err(port_err)
    }
}
