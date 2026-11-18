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
//! New-window policy: see `links`. JavaScript dialogs: see `dialogs`. Native menus and
//! tray: see `menus`. Quitting: see [`graceful_quit`]. What differs between macOS, Linux and
//! Windows: see `native`.
//!
//! [`WebviewHost`]: domain::ports::WebviewHost
//! [`Notifier`]: domain::ports::Notifier
//! [`Platform`]: domain::ports::Platform

mod data_stores;
mod dialogs;
mod downloads;
mod icons;
mod labels;
mod links;
mod logger;
mod menus;
mod native;
mod notifications;
mod page_hooks;
mod platform;
mod quit;
mod webviews;
#[cfg(not(target_os = "macos"))]
mod window_shortcuts;

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use domain::ports::{PortError, PortResult, UiSink};
use domain::{Bounds, Input};
use protocol::{EVENT_SNAPSHOT, Snapshot, UI_WEBVIEW};
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager, Runtime, Theme, WebviewBuilder, WebviewUrl, Window};

use crate::links::{PopupQuota, ViewPolicies};
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

/// Removes what the launcher put into the environment that would override the settings (the
/// `GTK_THEME` of an `AppImage` on Linux). Call it first in `main`, before any thread starts.
pub fn clean_launch_environment() {
    native::clean_launch_environment();
}

/// Names the app `identifier` for the desktop, so it matches the windows to Atrium's desktop
/// entry (Linux) or Start menu shortcut (Windows). Call it before the app is built.
pub fn set_app_id(identifier: &str) {
    native::set_app_id(identifier);
}

/// The host's part of the user agent presets (see `domain::webkit_user_agent`).
pub fn user_agent_platform() -> String {
    native::user_agent_platform()
}

/// The user agent preset of new accounts whose service asks for the `webkit` one, where the
/// engine is no `WebKit` (see `domain::Core::set_new_account_user_agent`).
pub fn new_account_user_agent() -> Option<String> {
    native::NEW_ACCOUNT_USER_AGENT.map(String::from)
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

/// Has the core reload a service view whose web content process ended. On macOS one hook of
/// `builder` covers every webview and reloads the others in place; on Linux and Windows each
/// service view is watched when it is created, so `builder` stays as it is.
pub fn register_crash_hook<R: Runtime>(
    builder: tauri::Builder<R>,
    feed: &Feed,
) -> tauri::Builder<R> {
    native::register_crash_hook(builder, feed)
}

/// Keeps Atrium to one running instance where the platform does not (all but macOS): a second
/// start brings the window `window_label` of the running one to the front and exits. Register
/// it before the other plugins.
pub fn guard_single_instance<R: Runtime>(
    builder: tauri::Builder<R>,
    window_label: &'static str,
) -> tauri::Builder<R> {
    native::guard_single_instance(builder, window_label)
}

/// Tells the desktop that Atrium added or removed icons of its desktop entry (Linux: KDE does
/// not notice new icon size folders by itself); the error, if any, as text for the log.
pub fn announce_new_icons() -> Result<(), String> {
    native::announce_new_icons()
}

/// The installed browsers to choose from for links, or `None` where the platform offers no list
/// (the browser is then typed by name).
pub fn installed_browsers() -> Option<Vec<protocol::BrowserChoice>> {
    native::installed_browsers()
}

/// Implements all webview-engine and OS ports with Tauri.
pub struct TauriHost<R: Runtime> {
    app: AppHandle<R>,
    window_label: String,
    feed: Feed,
    notifications: Arc<Mutex<NotificationTargets>>,
    logger: Logger,
    policies: ViewPolicies,
    dialogs: dialogs::Dialogs,
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
            dialogs: dialogs::Dialogs::new(),
            popup_counter: Arc::default(),
            popup_quota: Arc::default(),
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

    /// Calls `on_resume` each time the user is back at the computer, from a thread of the
    /// platform: on macOS when the displays wake, on Linux when the system resumed from suspend
    /// (logind), on Windows when it resumed from sleep or hibernation. Without a signal it logs
    /// why and never calls.
    pub fn observe_resume(&self, on_resume: impl Fn() + Send + Sync + 'static) {
        native::observe_resume(&self.logger, on_resume);
    }

    /// Makes `alert`, `confirm` and `prompt` of the service views show native panels. Call it
    /// once the sidebar webview exists and before the first service view is created (see
    /// `native::install_dialogs`); without it the pages get the default answers (macOS) or
    /// the engine's own dialogs (`WebKitGTK` on Linux, `WebView2` on Windows).
    pub fn install_dialogs(&self) {
        self.dialogs.install(&self.app, &self.logger);
    }

    /// Takes the account names for the titles of the pages' dialogs from `snapshot`; call it
    /// with each snapshot the core emits.
    pub fn set_dialog_names(&self, snapshot: &Snapshot) {
        self.dialogs.set_names(snapshot);
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
        Ok(store_fs::services::dir(&config_dir))
    }

    fn window(&self) -> PortResult<Window<R>> {
        self.app
            .get_window(&self.window_label)
            .ok_or_else(|| PortError(format!("window {:?} not found", self.window_label)))
    }
}

impl<R: Runtime> UiSink for TauriHost<R> {
    fn emit_snapshot(&self, snapshot: &Snapshot) -> PortResult {
        self.app
            .emit_to(UI_WEBVIEW, EVENT_SNAPSHOT, snapshot)
            .map_err(port_err)
    }
}
