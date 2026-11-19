//! Windows (`WebView2`): a `WebView2` user data folder per account (see `web_data`), child
//! webviews that Tauri lays out like on macOS (see `child_views`), the unread dot on the
//! taskbar button, Atrium's own JavaScript dialogs (`dialogs`), the browser list (`browsers`)
//! and the keyboard shortcuts of the main window's webviews (`shortcuts`). Resumes from sleep
//! come from the power notifications of Windows, crashed pages from each service view's
//! `ProcessFailed` event (see `platform_ffi::windows`).
//!
//! Environments: `WebView2` runs one browser process per user data folder and requires every
//! webview of a folder to be created with the same environment options, so Atrium sets none
//! per view. The sidebar keeps Tauri's folder (`EBWebView` in the app's local data folder);
//! popups run in their opener's environment (`WebviewWindowBuilder::window_features`).
//!
//! Hidden views: Tauri's background throttling setting is not supported by `WebView2`. Hiding
//! a child webview only turns its controller invisible (`IsVisible`), which `WebView2` does
//! not suspend (only `TrySuspend` does, never called): the page keeps running with the timer
//! throttling Chromium applies to hidden pages.
//!
//! Drag and drop: Tauri replaces the drop target of `WebView2` to report file drops to the
//! app, which takes HTML5 drag and drop from the pages. Atrium uses no file drops, so every
//! webview turns it off: the sidebar reorders its accounts by dragging, pages take dropped
//! uploads.
//!
//! Appearance: Tauri's theme needs nothing of Atrium's. For `Auto`, tao reads the app mode of
//! Windows (`AppsUseLightTheme`) at start and after each settings change, and Tauri sets the
//! preferred colour scheme of the window's webviews to it; a forced Light or Dark stays.

use domain::ports::PortResult;
use domain::{Bounds, DockBadge};
use platform_ffi::ProcessFailure;
use tauri::image::Image;
use tauri::webview::NewWindowFeatures;
use tauri::{AppHandle, Manager, Runtime, Webview, WebviewBuilder, WebviewWindowBuilder, Window};

use super::child_views::add_child_at;
use super::with_platform_webview;
use crate::labels::view_account;
use crate::{Feed, Input, Logger, port_err};

mod browsers;
mod dialogs;
mod shortcuts;

pub(crate) use crate::window_shortcuts::Shortcuts;
pub(crate) use browsers::{installed_browsers, open_url_with};
pub(crate) use dialogs::{attach_dialogs, install_dialogs};
pub(crate) use shortcuts::install_shortcuts;

/// Sound name of an instant message (`ms-winsoundevent:Notification.IM`), as
/// `tauri-winrt-notification` names it.
pub(crate) const NOTIFICATION_SOUND: &str = "IM";

/// Gives the process the `AppUserModelID` `identifier`, before any window exists: the taskbar
/// groups Atrium's windows by it, and a Start menu shortcut with the same id gives them its
/// name and icon. The notification plugin shows notifications under the same id, except for a
/// build in `target\debug` or `target\release`, whose notifications it shows as `PowerShell`'s.
pub(crate) fn set_app_id(identifier: &str) {
    // Fails only for an id longer than 128 characters, which a bundle identifier is not.
    let _ = platform_ffi::set_app_user_model_id(identifier);
}

/// Nothing to announce: Atrium installs no icons on Windows.
#[expect(clippy::unnecessary_wraps, reason = "same signature as on Linux")]
pub(crate) fn announce_new_icons() -> std::io::Result<()> {
    Ok(())
}

/// The platform part of the user agent presets, as the browsers report it on 64-bit Windows
/// (Windows 11 included).
pub(crate) fn user_agent_platform() -> String {
    "Windows NT 10.0; Win64; x64".into()
}

/// New accounts of services that ask for a `WebKit` browser's user agent present Chrome's:
/// `WebView2` is Chromium, and sites would otherwise serve it their code for Safari.
pub(crate) const NEW_ACCOUNT_USER_AGENT: Option<&str> = Some("chrome");

/// `WebView2` has no capture state: the service pages report it (service views only).
pub(crate) const PAGE_CAPTURE_DETECTION: bool = true;

/// The app icon in colour: Windows does not tint tray icons.
pub(crate) const TRAY_ICON: &[u8] = include_bytes!("../../../assets/tray-icon-color.png");
pub(crate) const TRAY_ICON_IS_TEMPLATE: bool = false;
/// The notification area takes a new menu at any time.
pub(crate) const TRAY_MENU_REPLACEABLE: bool = true;
/// No app menu: a menu bar would push the views down. Settings and Quit are in the tray.
pub(crate) const USES_APP_MENU: bool = false;
/// A popup runs its own user scripts: `WebView2` creates it as a webview of its own.
pub(crate) const POPUPS_RUN_OPENER_SCRIPTS: bool = false;

/// The red dot on the taskbar button while anything is unread.
const BADGE_DOT: &[u8] = include_bytes!("../../../assets/badge-dot.png");

/// Adds `builder` as a child webview of `window` at `bounds`, with the pages' own drag and drop
/// and the window's keyboard shortcuts.
pub(crate) fn add_child<R: Runtime>(
    window: &Window<R>,
    builder: WebviewBuilder<R>,
    bounds: Bounds,
) -> tauri::Result<Webview<R>> {
    let webview = add_child_at(window, builder.disable_drag_drop_handler(), bounds)?;
    shortcuts::attach(&webview);
    Ok(webview)
}

pub(crate) fn build_service_view<R: Runtime>(
    window: &Window<R>,
    builder: WebviewBuilder<R>,
    _store_id: [u8; 16],
    bounds: Bounds,
) -> PortResult<Webview<R>> {
    add_child(window, builder, bounds).map_err(port_err)
}

/// Reloads `webview` past the cache, through the `DevTools` protocol (the only way `WebView2`
/// offers); a failure is logged.
pub(crate) fn force_reload<R: Runtime>(webview: &Webview<R>, logger: &Logger) -> PortResult {
    let (label, logger) = (webview.label().to_string(), logger.clone());
    let report =
        move |error: String| logger.log(&format!("[{label}] force reload failed: {error}"));
    webview
        .with_webview(move |platform| {
            let controller = platform.controller();
            if let Err(error) = platform_ffi::reload_ignoring_cache(&controller, report.clone()) {
                report(error.to_string());
            }
        })
        .map_err(port_err)
}

/// Mutes or unmutes the audio of `webview`'s pages, keeping media capture as it is; a failure
/// (a `WebView2` runtime too old for it) is logged.
pub(crate) fn set_audio_muted<R: Runtime>(
    webview: &Webview<R>,
    muted: bool,
    logger: &Logger,
) -> PortResult {
    let (label, logger) = (webview.label().to_string(), logger.clone());
    webview
        .with_webview(move |platform| {
            if let Err(error) = platform_ffi::set_audio_muted(&platform.controller(), muted) {
                logger.log(&format!("[{label}] cannot mute audio: {error}"));
            }
        })
        .map_err(port_err)
}

/// Nothing to observe: `WebView2` tells no microphone or camera capture state (only that a
/// screen capture starts), so in Do Not Disturb a call's audio is muted like any other.
pub(crate) fn observe_capture<R: Runtime>(
    _webview: &Webview<R>,
    _on_change: super::OnCapture,
    _logger: &Logger,
) {
}

/// Gives the popup's page its own drag and drop. `WebView2` has no popup blocker: whether a
/// `window.open` came from a click is not checked yet, only the window quota applies.
pub(crate) fn prepare_popup<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    _features: &NewWindowFeatures,
) -> WebviewWindowBuilder<'a, R, M> {
    builder.disable_drag_drop_handler()
}

/// Calls `on_resume` after each resume from sleep or hibernation, on a thread of the system's:
/// twice, when the user woke the system, which the core takes for one resume. Without the power
/// notifications it logs why and never calls.
pub(crate) fn observe_resume(logger: &Logger, on_resume: impl Fn() + Send + Sync + 'static) {
    let resume_logger = logger.clone();
    let registered = platform_ffi::on_resume(move || {
        resume_logger.log("resumed from sleep");
        on_resume();
    });
    match registered {
        Ok(()) => logger.log("listening for resumes from sleep"),
        Err(error) => logger.log(&format!("no resume signal: {error}")),
    }
}

/// Nothing in the environment of a Windows launch overrides the theme or the other settings.
pub(crate) fn clean_launch_environment() {}

/// Shows a red dot on the taskbar button while anything is unread (a count or a dot), or
/// clears it.
pub(crate) fn set_badge<R: Runtime>(window: &Window<R>, badge: DockBadge) -> tauri::Result<()> {
    let unread = match badge {
        DockBadge::Count(count) => count > 0,
        DockBadge::Dot => true,
        DockBadge::None => false,
    };
    let dot = unread.then(|| Image::from_bytes(BADGE_DOT)).transpose()?;
    window.set_overlay_icon(dot)
}

/// Shows a warning with an OK button on the main thread (a Win32 message box).
pub(crate) fn show_warning<R: Runtime>(
    app: &AppHandle<R>,
    title: &str,
    text: &str,
) -> tauri::Result<()> {
    let (title, text) = (title.to_string(), text.to_string());
    app.run_on_main_thread(move || platform_ffi::show_warning(&title, &text))
}

/// Nothing to register for the whole app: Tauri's hook for terminated web processes does not
/// exist on Windows, [`observe_web_process_termination`] watches each service view.
pub(crate) fn register_crash_hook<R: Runtime>(
    builder: tauri::Builder<R>,
    _feed: &Feed,
) -> tauri::Builder<R> {
    builder
}

/// Has the core reload the service view `webview` when its page's render process ended
/// (`WebView2` shows its error page otherwise). Other failures are only logged: a page that
/// stopped responding may recover, and after its browser process ended the view is gone.
pub(crate) fn observe_web_process_termination<R: Runtime>(
    webview: &Webview<R>,
    feed: &Feed,
    logger: &Logger,
) {
    let label = webview.label().to_string();
    let (feed, event_logger) = (feed.clone(), logger.clone());
    let report = move |failure| {
        let Some(account) = view_account(&label) else {
            return;
        };
        event_logger.log(&format!("[{account}] web process failed: {failure:?}"));
        if reload_after(failure) {
            feed(Input::WebProcessTerminated { account });
        }
    };
    with_platform_webview(
        webview,
        logger,
        "cannot watch the web process",
        |platform| platform_ffi::on_process_failed(&platform.controller(), report),
    );
}

/// Whether a failed process is worth a reload: the page's render process ended.
fn reload_after(failure: ProcessFailure) -> bool {
    failure == ProcessFailure::RenderProcessExited
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_ended_render_process_is_reloaded() {
        assert!(reload_after(ProcessFailure::RenderProcessExited));
        for failure in [
            ProcessFailure::RenderProcessUnresponsive,
            ProcessFailure::BrowserProcessExited,
            ProcessFailure::Other,
        ] {
            assert!(!reload_after(failure), "{failure:?}");
        }
    }

    #[test]
    fn the_badge_dot_is_a_png() {
        assert!(Image::from_bytes(BADGE_DOT).is_ok());
    }
}
