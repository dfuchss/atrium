//! macOS: `WebKit` data stores per account, native JavaScript dialogs and a menu bar template
//! icon, through `platform_ffi` where Tauri has no API.

use std::convert::Infallible;
use std::sync::Arc;

use domain::ports::{PortError, PortResult};
use domain::{Bounds, DockBadge, MenuModel};
use platform_ffi::{DialogHandler, MainThreadMarker};
use protocol::{BrowserChoice, UI_WEBVIEW};
use tauri::webview::NewWindowFeatures;
use tauri::{
    AppHandle, Builder, Manager, Runtime, Webview, WebviewBuilder, WebviewWindowBuilder, Window,
};
use tauri_plugin_opener::OpenerExt;

pub(crate) use super::child_views::add_child_at as add_child;
use super::with_platform_webview;
use crate::labels::view_account;
use crate::{Feed, Input, Logger, port_err};

/// Sound name that makes `NSUserNotification` play the user's default notification sound
/// (mac-notification-sys maps exactly this string to `NSUserNotificationDefaultSoundName`;
/// other strings are looked up as named system sounds, unknown names play nothing).
pub(crate) const NOTIFICATION_SOUND: &str = "NSUserNotificationDefaultSoundName";

/// Nothing to do: macOS knows the app by its bundle.
pub(crate) fn set_app_id(_identifier: &str) {}

/// Nothing to announce: Atrium installs no icons on macOS.
#[expect(clippy::unnecessary_wraps, reason = "same signature as on Linux")]
pub(crate) fn announce_new_icons() -> Result<(), String> {
    Ok(())
}

/// The platform part of the user agent presets, as Safari reports it on every Mac, Apple
/// silicon included.
pub(crate) fn user_agent_platform() -> String {
    "Macintosh; Intel Mac OS X 10_15_7".into()
}

/// New accounts follow their service's user agent preset: the engine is `WebKit`.
pub(crate) const NEW_ACCOUNT_USER_AGENT: Option<&str> = None;

/// Monochrome template image (black + alpha); macOS tints it for light/dark menu bars.
pub(crate) const TRAY_ICON: &[u8] = include_bytes!("../../../assets/tray-icon.png");
pub(crate) const TRAY_ICON_IS_TEMPLATE: bool = true;
/// `NSStatusItem` takes a new menu at any time.
pub(crate) const TRAY_MENU_REPLACEABLE: bool = true;
/// The menu bar carries the app menu with the shortcuts.
pub(crate) const USES_APP_MENU: bool = true;
/// A popup runs its own user scripts.
pub(crate) const POPUPS_RUN_OPENER_SCRIPTS: bool = false;

/// Keeps the view's web data in the `WebKit` data store `store_id`.
#[expect(
    clippy::unnecessary_wraps,
    reason = "same signature as on Linux, where finding the data folder can fail"
)]
pub(crate) fn isolate_view<R: Runtime>(
    builder: WebviewBuilder<R>,
    _app: &AppHandle<R>,
    store_id: [u8; 16],
) -> tauri::Result<WebviewBuilder<R>> {
    Ok(builder.data_store_identifier(store_id))
}

/// Keeps the window's web data in the `WebKit` data store `store_id`.
#[expect(
    clippy::unnecessary_wraps,
    reason = "same signature as on Linux, where finding the data folder can fail"
)]
pub(crate) fn isolate_window<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    _app: &AppHandle<R>,
    store_id: [u8; 16],
) -> tauri::Result<WebviewWindowBuilder<'a, R, M>> {
    Ok(builder.data_store_identifier(store_id))
}

/// The data stores of this app.
pub(crate) fn data_store_ids<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Vec<[u8; 16]>> {
    tauri::async_runtime::block_on(app.fetch_data_store_identifiers())
}

/// Deletes the data store `store_id`; `WebKit` refuses while a live page uses it.
pub(crate) fn remove_data_store<R: Runtime>(
    app: &AppHandle<R>,
    store_id: [u8; 16],
) -> tauri::Result<()> {
    tauri::async_runtime::block_on(app.remove_data_store(store_id))
}

/// Adds a service view to `window`, on the main thread, with the `WebKit` configuration of
/// [`platform_ffi::service_configuration`]: it can only be created there and must be in place
/// before the page starts.
pub(crate) fn build_service_view<R: Runtime>(
    window: &Window<R>,
    builder: WebviewBuilder<R>,
    store_id: [u8; 16],
    bounds: Bounds,
) -> PortResult<Webview<R>> {
    MainThreadMarker::new()
        .ok_or_else(|| PortError("not on the main thread".into()))
        .and_then(|main_thread| {
            let configuration = platform_ffi::service_configuration(main_thread, store_id);
            add_child(
                window,
                builder.with_webview_configuration(configuration),
                bounds,
            )
            .map_err(port_err)
        })
}

/// Reloads `webview` from its origin, bypassing the cache.
pub(crate) fn force_reload<R: Runtime>(webview: &Webview<R>, _logger: &Logger) -> PortResult {
    webview
        .with_webview(|platform| platform_ffi::reload_from_origin(&platform))
        .map_err(port_err)
}

/// Like the account's view: `window.open` only from a click (see `platform_ffi`).
pub(crate) fn prepare_popup<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    features: &NewWindowFeatures,
) -> WebviewWindowBuilder<'a, R, M> {
    platform_ffi::forbid_automatic_windows(&features.opener().target_configuration);
    builder
}

/// Calls `on_resume` each time the displays wake, the sign that the user is back: a dark wake
/// (Power Nap) leaves them off. Nothing to log: the observer cannot fail.
pub(crate) fn observe_resume(_logger: &Logger, on_resume: impl Fn() + Send + Sync + 'static) {
    platform_ffi::observe_screens_wake(on_resume);
}

/// Leaves `builder` as it is: started again, macOS activates the running app itself (Launch
/// Services), and a development instance may run beside it.
pub(crate) fn guard_single_instance<R: Runtime>(
    builder: Builder<R>,
    _window_label: &'static str,
) -> Builder<R> {
    builder
}

/// Opens `url` in the app named `browser`, or in the default browser. With a browser this runs
/// `/usr/bin/open -a <browser> -- <url>` and waits for it.
pub(crate) fn open_url_with<R: Runtime>(
    app: &AppHandle<R>,
    url: &str,
    browser: Option<&str>,
    _logger: &Logger,
) -> PortResult {
    app.opener().open_url(url, browser).map_err(port_err)
}

/// No list: the settings take the browser's app name as text.
pub(crate) fn installed_browsers() -> Option<Vec<BrowserChoice>> {
    None
}

/// The keyboard shortcuts: the app menu carries them (see `menus`), so there is nothing to add.
pub(crate) struct Shortcuts;

impl Shortcuts {
    pub(crate) fn new() -> Shortcuts {
        Shortcuts
    }

    #[expect(clippy::unused_self, reason = "same methods as on Linux")]
    #[expect(
        clippy::unnecessary_wraps,
        reason = "same signature as on Linux, where adding the shortcuts can fail"
    )]
    pub(crate) fn install<R: Runtime>(
        &mut self,
        _app: &AppHandle<R>,
        _window_label: &str,
        _feed: &Feed,
    ) -> tauri::Result<()> {
        Ok(())
    }

    #[expect(clippy::unused_self, reason = "same methods as on Linux")]
    pub(crate) fn set_accounts(&self, _model: &MenuModel) {}
}

/// Nothing in the environment of a macOS launch overrides the settings.
pub(crate) fn clean_launch_environment() {}

/// Shows the unread count or a dot on the Dock icon, or clears it.
pub(crate) fn set_badge<R: Runtime>(window: &Window<R>, badge: DockBadge) -> tauri::Result<()> {
    match badge {
        DockBadge::Count(count) if count > 0 => window.set_badge_count(Some(i64::from(count))),
        DockBadge::Dot => window.set_badge_label(Some("•".to_string())),
        DockBadge::None | DockBadge::Count(_) => window.set_badge_label(None),
    }
}

/// Shows an app-modal warning with an OK button on the main thread (`NSAlert`).
pub(crate) fn show_warning<R: Runtime>(
    app: &AppHandle<R>,
    title: &str,
    text: &str,
) -> tauri::Result<()> {
    let (title, text) = (title.to_string(), text.to_string());
    app.run_on_main_thread(move || {
        if let Some(main_thread) = MainThreadMarker::new() {
            platform_ffi::show_warning(main_thread, &title, &text);
        }
    })
}

/// Makes `alert`, `confirm` and `prompt` of the service views show native panels decided by
/// `handler`. Needs the sidebar webview and must run before the first service view is created
/// (see `platform_ffi::install`).
pub(crate) fn install_dialogs<R: Runtime>(
    app: &AppHandle<R>,
    handler: Arc<dyn DialogHandler>,
    logger: &Logger,
) {
    let Some(sidebar) = app.get_webview(UI_WEBVIEW) else {
        logger.log("JavaScript dialogs unavailable: no sidebar webview yet");
        return;
    };
    let install_logger = logger.clone();
    with_platform_webview(
        &sidebar,
        logger,
        "JavaScript dialogs unavailable",
        move |platform| {
            platform_ffi::install(&platform, handler)
                .map(|()| install_logger.log("JavaScript dialogs installed"))
        },
    );
}

/// Replaces Tauri's default for a terminated web content process, which reloads the webview
/// without telling anyone: a service view is reloaded through the core, which logs it; other
/// webviews (sidebar, in-app windows) are reloaded in place as before.
pub(crate) fn register_crash_hook<R: Runtime>(
    builder: tauri::Builder<R>,
    feed: &Feed,
) -> tauri::Builder<R> {
    let feed = feed.clone();
    builder.on_web_content_process_terminate(move |webview| {
        match view_account(webview.label()) {
            Some(account) => feed(Input::WebProcessTerminated { account }),
            None => {
                // Nobody to tell: the hook has no logger, and the webview may already be
                // closing.
                let _ = webview.reload();
            }
        }
    })
}

/// Nothing to observe per view: [`register_crash_hook`] covers every webview.
pub(crate) fn observe_web_process_termination<R: Runtime>(
    _webview: &Webview<R>,
    _feed: &Feed,
    _logger: &Logger,
) {
}

/// Makes the dialogs of `webview` carry its label (see `labels::account_of_label`).
pub(crate) fn attach_dialogs<R: Runtime>(webview: &Webview<R>, logger: &Logger) {
    let label = webview.label().to_string();
    with_platform_webview(
        webview,
        logger,
        "cannot tag webview for dialogs",
        move |platform| {
            platform_ffi::tag_webview(&platform, &label);
            Ok::<(), Infallible>(())
        },
    );
}
