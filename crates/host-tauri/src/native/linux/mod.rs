//! Linux (`WebKitGTK` 4.1, GTK 3): a web data folder per account, child webviews laid out in a
//! `gtk::Layout`, and Atrium's own JavaScript dialogs (`dialogs`). Resumes from suspend
//! come from logind (`resume`). Keyboard shortcuts (`shortcuts`), the browser for links
//! (`browsers`) and the appearance (`theme`) have a module each.
//!
//! Layout: Tauri builds every child webview into the window's vertical `gtk::Box`, where wry
//! packs it to fill (the views would stack) and cannot move or resize it. So each new child
//! webview is moved into a `gtk::Layout` that fills that box (one per window, made on first
//! use) and placed there; unlike a `gtk::Fixed`, a layout lets the window shrink below its
//! children.
//!
//! Visibility: a window's `show` (tao: `gtk_widget_show_all`) would also show every hidden
//! view, above the sidebar (a layout draws its children in the order they were added) and so
//! above the settings dialog. So the adopted views do not take part in a `show_all`
//! (`no_show_all`) and [`set_visible`] shows and hides them itself, since wry shows with
//! `show_all` too.

use std::time::Duration;

use domain::ports::PortResult;
use domain::{Bounds, DockBadge};
use gtk::prelude::*;
use tauri::webview::NewWindowFeatures;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, Webview, WebviewBuilder,
    WebviewWindowBuilder, Window,
};
use webkit2gtk::{WebProcessTerminationReason, WebViewExt};

use crate::webviews::view_account;
use crate::{Feed, Input, Logger, port_err};

mod browsers;
mod dialogs;
mod resume;
mod shortcuts;
mod theme;

pub(crate) use browsers::{installed_browsers, open_url_with};
pub(crate) use dialogs::{attach_dialogs, install_dialogs};
pub(crate) use resume::observe_resume;
pub(crate) use shortcuts::Shortcuts;
pub(crate) use theme::clean_launch_environment;

/// How long a D-Bus method call may take; a desktop without the service answers at once, a
/// hanging one must not keep the caller waiting.
const BUS_TIMEOUT: Duration = Duration::from_secs(1);

/// A session bus connection whose method calls time out: a stuck bus must not freeze the caller.
fn session_connection() -> zbus::Result<zbus::blocking::Connection> {
    zbus::blocking::connection::Builder::session()?
        .method_timeout(BUS_TIMEOUT)
        .build()
}

/// Freedesktop sound theme name of an instant message.
pub(crate) const NOTIFICATION_SOUND: &str = "message-new-instant";

/// Sets the program name of `GLib` to `identifier` before GTK starts. GTK takes the window's
/// Wayland app id and the instance of its X11 `WM_CLASS` from it, which the desktop matches to
/// the entry `<identifier>.desktop` or to an entry with `StartupWMClass=<identifier>`.
pub(crate) fn set_app_id(identifier: &str) {
    gtk::glib::set_prgname(Some(identifier));
}

/// Tells the running KDE programs (Plasma's app menu, `KWin`) that icons were added or removed:
/// they read the icon folders once and do not see a size folder created later. Elsewhere nobody
/// listens; GTK notices new icons by itself.
pub(crate) fn announce_new_icons() -> Result<(), String> {
    emit_icon_changed().map_err(|error| error.to_string())
}

fn emit_icon_changed() -> zbus::Result<()> {
    let connection = session_connection()?;
    connection.emit_signal(
        None::<zbus::names::BusName<'_>>,
        "/KIconLoader",
        "org.kde.KIconLoader",
        "iconChanged",
        // The icon group; every group reloads.
        &0_i32,
    )
}

/// The platform part of the user agent presets, as `WebKitGTK` reports it: X11 and the
/// processor architecture.
pub(crate) fn user_agent_platform() -> String {
    format!("X11; Linux {}", std::env::consts::ARCH)
}

/// The app icon in colour: panels are light or dark and do not tint tray icons.
pub(crate) const TRAY_ICON: &[u8] = include_bytes!("../../../assets/tray-icon-color.png");
pub(crate) const TRAY_ICON_IS_TEMPLATE: bool = false;
/// An `AppIndicator` keeps the first menu it is given; only its items can change.
pub(crate) const TRAY_MENU_REPLACEABLE: bool = false;
/// No app menu: a GTK menu bar would push the views down. Settings and Quit are in the tray.
pub(crate) const USES_APP_MENU: bool = false;
/// A popup (a related view of its opener) runs its opener's user scripts, not its own.
pub(crate) const POPUPS_RUN_OPENER_SCRIPTS: bool = true;

/// Logical pixels as GTK takes them.
#[expect(
    clippy::cast_possible_truncation,
    reason = "window coordinates are far inside the i32 range"
)]
fn px(logical: f64) -> i32 {
    logical.round() as i32
}

/// The `gtk::Layout` in `vbox`, made and packed to fill it on first use.
fn layout_in(vbox: &gtk::Box) -> gtk::Layout {
    let existing = vbox
        .children()
        .into_iter()
        .find_map(|child| child.downcast::<gtk::Layout>().ok());
    existing.unwrap_or_else(|| {
        let layout = gtk::Layout::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        vbox.pack_start(&layout, true, true, 0);
        layout.show();
        layout
    })
}

/// Moves `widget` to `bounds` in its layout and gives it that size.
fn put(widget: &gtk::Widget, layout: &gtk::Layout, bounds: Bounds) {
    layout.move_(widget, px(bounds.x), px(bounds.y));
    widget.set_size_request(px(bounds.width), px(bounds.height));
}

/// Moves `webview` from the window's box, where wry packed it, into the window's layout at
/// `bounds` (on the main thread, like every webview call).
fn adopt<R: Runtime>(webview: &Webview<R>, bounds: Bounds) -> tauri::Result<()> {
    webview.with_webview(move |platform| {
        let widget = platform.inner().upcast::<gtk::Widget>();
        let Some(vbox) = widget
            .parent()
            .and_then(|parent| parent.downcast::<gtk::Box>().ok())
        else {
            return;
        };
        let layout = layout_in(&vbox);
        vbox.remove(&widget);
        widget.set_no_show_all(true);
        layout.put(&widget, px(bounds.x), px(bounds.y));
        widget.set_size_request(px(bounds.width), px(bounds.height));
    })
}

/// Adds `builder` as a child webview of `window` at `bounds`.
pub(crate) fn add_child<R: Runtime>(
    window: &Window<R>,
    builder: WebviewBuilder<R>,
    bounds: Bounds,
) -> tauri::Result<Webview<R>> {
    let webview = window.add_child(
        builder,
        LogicalPosition::new(bounds.x, bounds.y),
        LogicalSize::new(bounds.width, bounds.height),
    )?;
    adopt(&webview, bounds)?;
    Ok(webview)
}

/// Whether a terminated web process is worth a reload: not when Atrium ended it itself.
fn reload_after(reason: WebProcessTerminationReason) -> bool {
    !matches!(reason, WebProcessTerminationReason::TerminatedByApi)
}

/// Nothing to register for the whole app: [`observe_web_process_termination`] watches each
/// service view.
pub(crate) fn register_crash_hook<R: Runtime>(
    builder: tauri::Builder<R>,
    _feed: &Feed,
) -> tauri::Builder<R> {
    builder
}

/// Has the core reload the service view `webview` when its web process crashes or is killed
/// for its memory (`WebKitGTK` leaves the view blank otherwise). A process Atrium ended itself
/// is left alone.
pub(crate) fn observe_web_process_termination<R: Runtime>(
    webview: &Webview<R>,
    feed: &Feed,
    logger: &Logger,
) {
    let label = webview.label().to_string();
    let (feed, signal_logger) = (feed.clone(), logger.clone());
    let connected = webview.with_webview(move |platform| {
        platform
            .inner()
            .connect_web_process_terminated(move |_, reason| {
                let Some(account) = view_account(&label).filter(|_| reload_after(reason)) else {
                    return;
                };
                signal_logger.log(&format!("[{account}] web process ended: {reason:?}"));
                feed(Input::WebProcessTerminated { account });
            });
    });
    if let Err(error) = connected {
        logger.log(&format!(
            "[{}] cannot watch the web process: {error}",
            webview.label()
        ));
    }
}

/// Adds a service view to `window` (called on the main thread).
pub(crate) fn build_service_view<R: Runtime>(
    window: &Window<R>,
    builder: WebviewBuilder<R>,
    _store_id: [u8; 16],
    bounds: Bounds,
) -> PortResult<Webview<R>> {
    add_child(window, builder, bounds).map_err(port_err)
}

/// Moves and resizes the child webview `webview` to `bounds` in its layout.
pub(crate) fn place<R: Runtime>(webview: &Webview<R>, bounds: Bounds) -> PortResult {
    webview
        .with_webview(move |platform| {
            let widget = platform.inner().upcast::<gtk::Widget>();
            if let Some(layout) = widget
                .parent()
                .and_then(|parent| parent.downcast::<gtk::Layout>().ok())
            {
                put(&widget, &layout, bounds);
            }
        })
        .map_err(port_err)
}

/// Shows or hides the child webview `webview` on the widget itself: it is `no_show_all`, which
/// Tauri's `show` (wry: `gtk_widget_show_all`) leaves alone.
pub(crate) fn set_visible<R: Runtime>(webview: &Webview<R>, visible: bool) -> PortResult {
    webview
        .with_webview(move |platform| platform.inner().set_visible(visible))
        .map_err(port_err)
}

/// Reloads `webview` bypassing the cache: every resource is revalidated with the server.
pub(crate) fn force_reload<R: Runtime>(webview: &Webview<R>, _logger: &Logger) -> PortResult {
    webview
        .with_webview(|platform| platform.inner().reload_bypass_cache())
        .map_err(port_err)
}

/// Calls `decide` once with the size the page asked for `window` (`window.open` features), or
/// `None` if it asked for none, as soon as `WebKitGTK` tells it: when the window is ready to
/// show, before its page loads. wry's new-window request carries no size on Linux. On the main
/// thread (as inside wry's new-window callback) the handler is in place before this returns.
///
/// `WebKitGTK` sets a new window's geometry to the window's own size unless the page asked for
/// one, so `window` must be sized to something a page is unlikely to ask for: see
/// [`UNDECIDED_WINDOW_SIZE`].
pub(crate) fn on_requested_size<R: Runtime>(
    window: &tauri::WebviewWindow<R>,
    decide: impl FnOnce(Option<LogicalSize<f64>>) + Send + 'static,
) -> tauri::Result<()> {
    window.with_webview(move |platform| {
        let decide = std::cell::Cell::new(Some(decide));
        platform.inner().connect_ready_to_show(move |view| {
            if let Some(decide) = decide.take() {
                decide(requested_size(view));
            }
        });
    })
}

/// Size of a window whose requested size is not known yet: `WebKit`'s smallest window, so only
/// a page asking for at most that size is taken for one that asked for none.
pub(crate) const UNDECIDED_WINDOW_SIZE: (f64, f64) = (100.0, 100.0);

/// The window size `view`'s page asked for: its geometry, unless that is just the size of the
/// window it is in.
fn requested_size(view: &webkit2gtk::WebView) -> Option<LogicalSize<f64>> {
    use webkit2gtk::WindowPropertiesExt;

    let geometry = view.window_properties()?.geometry();
    let requested = (geometry.width(), geometry.height());
    let own = view
        .toplevel()
        .and_then(|toplevel| toplevel.downcast::<gtk::Window>().ok())
        .map(|window| window.size());
    (own != Some(requested))
        .then(|| LogicalSize::new(f64::from(requested.0), f64::from(requested.1)))
}

/// Nothing to prepare: `WebKitGTK`'s `javascript-can-open-windows-automatically` is off by
/// default and wry leaves it, so pages open windows only from a user gesture.
pub(crate) fn prepare_popup<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    _features: &NewWindowFeatures,
) -> WebviewWindowBuilder<'a, R, M> {
    builder
}

/// Shows the unread count on the launcher icon (Unity launcher API), or clears it; a dot
/// badge shows nothing.
pub(crate) fn set_badge<R: Runtime>(window: &Window<R>, badge: DockBadge) -> tauri::Result<()> {
    match badge {
        DockBadge::Count(count) if count > 0 => window.set_badge_count(Some(i64::from(count))),
        DockBadge::Dot | DockBadge::None | DockBadge::Count(_) => window.set_badge_count(None),
    }
}

/// Shows a modal warning with an OK button on the main thread (`gtk::MessageDialog`).
pub(crate) fn show_warning<R: Runtime>(
    app: &AppHandle<R>,
    title: &str,
    text: &str,
) -> tauri::Result<()> {
    let (title, text) = (title.to_string(), text.to_string());
    app.run_on_main_thread(move || {
        let dialog = gtk::MessageDialog::new(
            None::<&gtk::Window>,
            gtk::DialogFlags::MODAL,
            gtk::MessageType::Warning,
            gtk::ButtonsType::Ok,
            &title,
        );
        dialog.set_title("Atrium");
        dialog.set_secondary_text(Some(&text));
        dialog.run();
        dialog.close();
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_process_atrium_did_not_end_is_reloaded() {
        assert!(reload_after(WebProcessTerminationReason::Crashed));
        assert!(reload_after(
            WebProcessTerminationReason::ExceededMemoryLimit
        ));
        assert!(!reload_after(WebProcessTerminationReason::TerminatedByApi));
    }
}
