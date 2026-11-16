//! macOS: `WebKit` data stores per account, native JavaScript dialogs and a menu bar template
//! icon, through `webview_dialogs` where Tauri has no API.

use std::sync::Arc;

use domain::ports::{PortError, PortResult};
use domain::{Bounds, DockBadge};
use protocol::UI_WEBVIEW;
use tauri::webview::NewWindowFeatures;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, Webview, WebviewBuilder,
    WebviewWindowBuilder, Window,
};
use tauri_plugin_opener::OpenerExt;
use webview_dialogs::{DialogHandler, MainThreadMarker};

use crate::webviews::view_account;
use crate::{Feed, Input, Logger, port_err};

/// Sound name that makes `NSUserNotification` play the user's default notification sound
/// (mac-notification-sys maps exactly this string to `NSUserNotificationDefaultSoundName`;
/// other strings are looked up as named system sounds, unknown names play nothing).
pub(crate) const NOTIFICATION_SOUND: &str = "NSUserNotificationDefaultSoundName";

/// Monochrome template image (black + alpha); macOS tints it for light/dark menu bars.
pub(crate) const TRAY_ICON: &[u8] = include_bytes!("../../../assets/tray-icon.png");
pub(crate) const TRAY_ICON_IS_TEMPLATE: bool = true;

/// Keeps the view's web data in the `WebKit` data store `store_id`.
pub(crate) fn isolate_view<R: Runtime>(
    builder: WebviewBuilder<R>,
    store_id: [u8; 16],
) -> WebviewBuilder<R> {
    builder.data_store_identifier(store_id)
}

/// Keeps the window's web data in the `WebKit` data store `store_id`.
pub(crate) fn isolate_window<R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'_, R, M>,
    store_id: [u8; 16],
) -> WebviewWindowBuilder<'_, R, M> {
    builder.data_store_identifier(store_id)
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

/// Adds `builder` as a child webview of `window` at `bounds`.
pub(crate) fn add_child<R: Runtime>(
    window: &Window<R>,
    builder: WebviewBuilder<R>,
    bounds: Bounds,
) -> tauri::Result<Webview<R>> {
    window.add_child(
        builder,
        LogicalPosition::new(bounds.x, bounds.y),
        LogicalSize::new(bounds.width, bounds.height),
    )
}

/// Adds a service view to `window`, on the main thread, with the `WebKit` configuration of
/// [`webview_dialogs::service_configuration`]: it can only be created there and must be in place
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
            let configuration = webview_dialogs::service_configuration(main_thread, store_id);
            add_child(
                window,
                builder.with_webview_configuration(configuration),
                bounds,
            )
            .map_err(port_err)
        })
}

/// Moves and resizes the child webview `webview` to `bounds`.
pub(crate) fn place<R: Runtime>(webview: &Webview<R>, bounds: Bounds) -> PortResult {
    webview
        .set_position(LogicalPosition::new(bounds.x, bounds.y))
        .map_err(port_err)?;
    webview
        .set_size(LogicalSize::new(bounds.width, bounds.height))
        .map_err(port_err)
}

/// Shows or hides the child webview `webview`.
pub(crate) fn set_visible<R: Runtime>(webview: &Webview<R>, visible: bool) -> PortResult {
    if visible {
        webview.show().map_err(port_err)
    } else {
        webview.hide().map_err(port_err)
    }
}

/// Reloads `webview` from its origin, bypassing the cache.
pub(crate) fn force_reload<R: Runtime>(webview: &Webview<R>) -> PortResult {
    webview
        .with_webview(|platform| webview_dialogs::reload_from_origin(&platform))
        .map_err(port_err)
}

/// Like the account's view: `window.open` only from a click (see `webview_dialogs`).
pub(crate) fn prepare_popup(features: &NewWindowFeatures) {
    webview_dialogs::forbid_automatic_windows(&features.opener().target_configuration);
}

/// Calls `on_wake` each time the displays wake (not on a Power Nap dark wake).
pub(crate) fn observe_screens_wake(on_wake: impl Fn() + Send + Sync + 'static) {
    webview_dialogs::observe_screens_wake(on_wake);
}

/// Opens `url` in the app named `browser`, or in the default browser. With a browser this runs
/// `/usr/bin/open -a <browser> -- <url>` and waits for it.
pub(crate) fn open_url_with<R: Runtime>(
    app: &AppHandle<R>,
    url: &str,
    browser: Option<&str>,
) -> PortResult {
    app.opener().open_url(url, browser).map_err(port_err)
}

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
            webview_dialogs::show_warning(main_thread, &title, &text);
        }
    })
}

/// Makes `alert`, `confirm` and `prompt` of the service views show native panels decided by
/// `handler`. Needs the sidebar webview and must run before the first service view is created
/// (see `webview_dialogs::install`).
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
    let scheduled =
        sidebar.with_webview(
            move |platform| match webview_dialogs::install(&platform, handler) {
                Ok(()) => install_logger.log("JavaScript dialogs installed"),
                Err(error) => {
                    install_logger.log(&format!("JavaScript dialogs unavailable: {error}"));
                }
            },
        );
    if let Err(error) = scheduled {
        logger.log(&format!("JavaScript dialogs unavailable: {error}"));
    }
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

/// Makes the dialogs of `webview` carry its label (see `dialogs::account_of_label`).
pub(crate) fn attach_dialogs<R: Runtime>(webview: &Webview<R>, logger: &Logger) {
    let label = webview.label().to_string();
    let tagged =
        webview.with_webview(move |platform| webview_dialogs::tag_webview(&platform, &label));
    if let Err(error) = tagged {
        logger.log(&format!(
            "[{}] cannot tag webview for dialogs: {error}",
            webview.label()
        ));
    }
}
