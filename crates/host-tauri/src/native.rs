//! What differs between the platforms Atrium runs on: `macos`, `linux` and `windows` export
//! the same items, listed with their types in [`_platform_interface`], so an item missing or
//! typed differently on one platform fails the build on every platform, not only in that
//! platform's CI. Everything else of the host is shared. What the platforms other than macOS
//! share lives here too: the per-account web data folders (`web_data`) and the single-instance
//! guard (`single_instance`); macOS and Windows share the child webviews Tauri lays out by
//! itself (`child_views`).
//!
//! [`with_platform_webview`] runs a job on the engine's own webview, for the calls Tauri has no
//! API for.

use std::fmt::Display;
use std::sync::Arc;

use domain::ports::PortResult;
use domain::{Bounds, DockBadge, MenuModel};
use platform_ffi::DialogHandler;
use protocol::BrowserChoice;
use tauri::webview::{NewWindowFeatures, PlatformWebview};
use tauri::{
    AppHandle, Builder, Manager, Runtime, Webview, WebviewBuilder, WebviewWindowBuilder, Window,
};

use crate::{Feed, Logger};

#[cfg(not(target_os = "linux"))]
mod child_views;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "linux", windows))]
mod single_instance;
#[cfg(not(target_os = "macos"))]
mod web_data;
#[cfg(windows)]
mod windows;

#[cfg(not(target_os = "linux"))]
pub(crate) use child_views::{place, set_visible};
#[cfg(target_os = "linux")]
pub(crate) use linux::*;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;
#[cfg(any(target_os = "linux", windows))]
pub(crate) use single_instance::guard_single_instance;
#[cfg(not(target_os = "macos"))]
pub(crate) use web_data::{data_store_ids, isolate_view, isolate_window, remove_data_store};
#[cfg(windows)]
pub(crate) use windows::*;

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
compile_error!("Atrium runs on macOS, Linux and Windows only");

/// Called on the main thread when a webview's page starts (true) or stops capturing the
/// microphone or camera (see `observe_capture`).
pub(crate) type OnCapture = Box<dyn Fn(bool) + Send>;

/// Runs `job` with the engine's webview of `webview` on the main thread; a failure of the job,
/// or to schedule it, is logged as `[<label>] <failure>: <error>`.
pub(crate) fn with_platform_webview<R: Runtime, E: Display>(
    webview: &Webview<R>,
    logger: &Logger,
    failure: &'static str,
    job: impl FnOnce(PlatformWebview) -> Result<(), E> + Send + 'static,
) {
    let (label, job_logger) = (webview.label().to_string(), logger.clone());
    let scheduled = webview.with_webview(move |platform| {
        if let Err(error) = job(platform) {
            job_logger.log(&format!("[{label}] {failure}: {error}"));
        }
    });
    if let Err(error) = scheduled {
        logger.log(&format!("[{}] {failure}: {error}", webview.label()));
    }
}

/// The items each platform module exports, by concern, coerced to their types. Never called:
/// type-checking the body is the test.
#[expect(clippy::type_complexity, reason = "the types are the interface")]
fn _platform_interface<R: Runtime, M: Manager<R>>() {
    // Web data: each account's views and windows keep their cookies and storage apart; what is
    // kept per account can be listed and deleted.
    let _: fn(WebviewBuilder<R>, &AppHandle<R>, [u8; 16]) -> tauri::Result<WebviewBuilder<R>> =
        isolate_view;
    let _: for<'a> fn(
        WebviewWindowBuilder<'a, R, M>,
        &AppHandle<R>,
        [u8; 16],
    ) -> tauri::Result<WebviewWindowBuilder<'a, R, M>> = isolate_window;
    let _: fn(&AppHandle<R>) -> tauri::Result<Vec<[u8; 16]>> = data_store_ids;
    let _: fn(&AppHandle<R>, [u8; 16]) -> tauri::Result<()> = remove_data_store;
    // Child webviews: the sidebar and the service views in the main window.
    let _: fn(&Window<R>, WebviewBuilder<R>, Bounds) -> tauri::Result<Webview<R>> = add_child;
    let _: fn(&Window<R>, WebviewBuilder<R>, [u8; 16], Bounds) -> PortResult<Webview<R>> =
        build_service_view;
    let _: fn(&Webview<R>, Bounds) -> PortResult = place;
    let _: fn(&Webview<R>, bool) -> PortResult = set_visible;
    let _: fn(&Webview<R>, &Logger) -> PortResult = force_reload;
    // Audio muted while Do Not Disturb is on, media capture untouched (service views, popups),
    // except in a webview that captures the microphone or camera (a call), where the engine
    // tells it (not `WebView2`).
    let _: fn(&Webview<R>, bool, &Logger) -> PortResult = set_audio_muted;
    let _: fn(&Webview<R>, OnCapture, &Logger) = observe_capture;
    // Popups: what a popup's page may do before it is created, whose user scripts it runs.
    let _: for<'a> fn(
        WebviewWindowBuilder<'a, R, M>,
        &NewWindowFeatures,
    ) -> WebviewWindowBuilder<'a, R, M> = prepare_popup;
    let _: bool = POPUPS_RUN_OPENER_SCRIPTS;
    // JavaScript dialogs of the service views and popups, as the `dialogs` policy decides.
    let _: fn(&AppHandle<R>, Arc<dyn DialogHandler>, &Logger) = install_dialogs;
    let _: fn(&Webview<R>, &Logger) = attach_dialogs;
    // Crashed web processes: the core reloads the service view, hooked for every webview
    // (macOS) or per service view (Linux and Windows).
    let _: fn(Builder<R>, &Feed) -> Builder<R> = register_crash_hook;
    let _: fn(&Webview<R>, &Feed, &Logger) = observe_web_process_termination;
    // User agents: the platform part of the presets, the preset of new accounts.
    let _: fn() -> String = user_agent_platform;
    let _: Option<&str> = NEW_ACCOUNT_USER_AGENT;
    let _: bool = PAGE_CAPTURE_DETECTION;
    // OS integration.
    let _: fn(&str) = set_app_id;
    let _: fn(&Window<R>, DockBadge) -> tauri::Result<()> = set_badge;
    let _: fn(&AppHandle<R>, &str, &str) -> tauri::Result<()> = show_warning;
    let _: fn(&Logger, fn()) = observe_resume;
    let _: fn(Builder<R>, &'static str) -> Builder<R> = guard_single_instance;
    let _: fn() -> std::io::Result<()> = announce_new_icons;
    let _: &str = NOTIFICATION_SOUND;
    let _: (&[u8], bool, bool) = (TRAY_ICON, TRAY_ICON_IS_TEMPLATE, TRAY_MENU_REPLACEABLE);
    // The keyboard shortcuts: carried by the app menu if there is one, else mapped to keys.
    let _: bool = USES_APP_MENU;
    let _: fn() -> Shortcuts = Shortcuts::new;
    let _: fn(&mut Shortcuts, &AppHandle<R>, &str, &Feed) -> tauri::Result<()> = Shortcuts::install;
    let _: fn(&Shortcuts, &MenuModel) = Shortcuts::set_accounts;
    // Settings: links in the chosen browser, the browsers to choose from, and no launcher
    // environment overriding the theme.
    let _: fn(&AppHandle<R>, &str, Option<&str>, &Logger) -> PortResult = open_url_with;
    let _: fn() -> Option<Vec<BrowserChoice>> = installed_browsers;
    let _: fn() = clean_launch_environment;
    // Only where there is no app menu: the shortcuts mapped to keys (`window_shortcuts`).
    #[cfg(not(target_os = "macos"))]
    let _: fn(crate::window_shortcuts::ShortcutActions<R>) -> tauri::Result<()> = install_shortcuts;
    // Only on Linux, whose new-window requests carry no size (`links`).
    #[cfg(target_os = "linux")]
    let _: fn(
        &tauri::WebviewWindow<R>,
        fn(Option<tauri::LogicalSize<f64>>),
    ) -> tauri::Result<()> = on_requested_size;
    #[cfg(target_os = "linux")]
    let _: (f64, f64) = UNDECIDED_WINDOW_SIZE;
}
