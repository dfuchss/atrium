//! Linux (`WebKitGTK` 4.1, GTK 3): a web data folder per account, child webviews laid out in a
//! `gtk::Layout`, and Atrium's own JavaScript dialogs (`dialogs`). A second start hands over to
//! the running Atrium (`single_instance`). Keyboard shortcuts (`shortcuts`), the browser for
//! links (`browsers`) and the appearance (`theme`) have a module each.
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

use std::fs::{DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use domain::ports::PortResult;
use domain::{Bounds, DockBadge};
use gtk::prelude::*;
use tauri::webview::NewWindowFeatures;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, Webview, WebviewBuilder,
    WebviewWindowBuilder, Window,
};
use uuid::Uuid;
use webkit2gtk::WebViewExt;

use crate::port_err;

mod browsers;
mod dialogs;
mod shortcuts;
mod single_instance;
mod theme;

pub(crate) use browsers::{installed_browsers, open_url_with};
pub(crate) use dialogs::{attach_dialogs, install_dialogs};
pub(crate) use shortcuts::Shortcuts;
pub(crate) use single_instance::guard_single_instance;
pub(crate) use theme::clean_launch_environment;

/// Freedesktop sound theme name of an instant message.
pub(crate) const NOTIFICATION_SOUND: &str = "message-new-instant";

/// Sets the program name of `GLib` to `identifier` before GTK starts. GTK takes the window's
/// Wayland app id and the instance of its X11 `WM_CLASS` from it, which the desktop matches to
/// the entry `<identifier>.desktop` or to an entry with `StartupWMClass=<identifier>`.
pub(crate) fn set_app_id(identifier: &str) {
    gtk::glib::set_prgname(Some(identifier));
}

/// The platform part of the user agent presets, as `WebKitGTK` reports it: X11 and the
/// processor architecture.
pub(crate) fn user_agent_platform() -> String {
    format!("X11; Linux {}", std::env::consts::ARCH)
}

/// The app icon in colour: panels are light or dark and do not tint tray icons.
pub(crate) const TRAY_ICON: &[u8] = include_bytes!("../../../assets/tray-icon-linux.png");
pub(crate) const TRAY_ICON_IS_TEMPLATE: bool = false;
/// An `AppIndicator` keeps the first menu it is given; only its items can change.
pub(crate) const TRAY_MENU_REPLACEABLE: bool = false;
/// No app menu: a GTK menu bar would push the views down. Settings and Quit are in the tray.
pub(crate) const USES_APP_MENU: bool = false;
/// A popup (a related view of its opener) runs its opener's user scripts, not its own.
pub(crate) const POPUPS_RUN_OPENER_SCRIPTS: bool = true;

/// Folder of the per-account web data folders, `<app data>/webdata`. A development config
/// folder (`ATRIUM_CONFIG_DIR`) does not move it, just as the data stores of macOS belong to
/// the app: the startup sweep's guards keep protecting the real data.
fn web_data_root<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    Ok(app.path().app_data_dir()?.join("webdata"))
}

/// Web data folder of the store `store_id` (an account id) in `root`.
fn store_dir(root: &Path, store_id: [u8; 16]) -> PathBuf {
    root.join(Uuid::from_bytes(store_id).hyphenated().to_string())
}

/// Mode of the web data folders: only the user may enter them.
const PRIVATE_DIR_MODE: u32 = 0o700;

/// Web data folder of `store_id` in `root`, made private (`0700`, like `root`) before
/// `WebKitGTK` writes to it: it keeps the account's cookies in a plain text file. Folders of
/// earlier runs are tightened too.
fn private_store_dir(root: &Path, store_id: [u8; 16]) -> io::Result<PathBuf> {
    let dir = store_dir(root, store_id);
    for folder in [root, dir.as_path()] {
        DirBuilder::new()
            .recursive(true)
            .mode(PRIVATE_DIR_MODE)
            .create(folder)?;
        std::fs::set_permissions(folder, Permissions::from_mode(PRIVATE_DIR_MODE))?;
    }
    Ok(dir)
}

/// The stores in `root`: its folders named by a hyphenated UUID; anything else is ignored. A
/// missing `root` holds none.
pub(crate) fn list_store_dirs(root: &Path) -> io::Result<Vec<[u8; 16]>> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|name| Uuid::try_parse(name).ok()) else {
            continue;
        };
        if store_dir(root, id.into_bytes()) == entry.path() {
            ids.push(id.into_bytes());
        }
    }
    Ok(ids)
}

/// Deletes the web data folder of `store_id` in `root`; one that is already gone counts as
/// deleted.
pub(crate) fn remove_store_dir(root: &Path, store_id: [u8; 16]) -> io::Result<()> {
    match std::fs::remove_dir_all(store_dir(root, store_id)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// Keeps the view's web data in the account's own folder (a `WebContext` of its own).
pub(crate) fn isolate_view<R: Runtime>(
    builder: WebviewBuilder<R>,
    app: &AppHandle<R>,
    store_id: [u8; 16],
) -> tauri::Result<WebviewBuilder<R>> {
    Ok(builder.data_directory(private_store_dir(&web_data_root(app)?, store_id)?))
}

/// Keeps the window's web data in the account's folder, the same `WebContext` as its view.
pub(crate) fn isolate_window<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    app: &AppHandle<R>,
    store_id: [u8; 16],
) -> tauri::Result<WebviewWindowBuilder<'a, R, M>> {
    Ok(builder.data_directory(private_store_dir(&web_data_root(app)?, store_id)?))
}

/// The accounts that have a web data folder.
pub(crate) fn data_store_ids<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Vec<[u8; 16]>> {
    Ok(list_store_dirs(&web_data_root(app)?)?)
}

/// Deletes the web data folder of `store_id`. Tauri keeps a `WebContext` alive until the app
/// exits (tauri#14626), so `WebKit` may still write into the folder of a just-closed account;
/// what it leaves is removed again at the next start.
pub(crate) fn remove_data_store<R: Runtime>(
    app: &AppHandle<R>,
    store_id: [u8; 16],
) -> tauri::Result<()> {
    Ok(remove_store_dir(&web_data_root(app)?, store_id)?)
}

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
pub(crate) fn force_reload<R: Runtime>(webview: &Webview<R>) -> PortResult {
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
pub(crate) fn prepare_popup(_features: &NewWindowFeatures) {}

/// No wake signal yet: nothing is called.
pub(crate) fn observe_screens_wake(_on_wake: impl Fn() + Send + Sync + 'static) {}

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
    fn stores_are_the_uuid_folders() {
        let root = tempfile::tempdir().unwrap();
        let (kept, removed) = (Uuid::from_u128(1), Uuid::from_u128(2));
        for id in [kept, removed] {
            std::fs::create_dir(store_dir(root.path(), id.into_bytes())).unwrap();
        }
        std::fs::create_dir(root.path().join(kept.simple().to_string())).unwrap();
        std::fs::create_dir(root.path().join("not-a-store")).unwrap();
        std::fs::write(root.path().join(Uuid::from_u128(3).to_string()), "x").unwrap();
        let mut listed = list_store_dirs(root.path()).unwrap();
        listed.sort_unstable();
        assert_eq!(listed, vec![kept.into_bytes(), removed.into_bytes()]);

        remove_store_dir(root.path(), removed.into_bytes()).unwrap();
        remove_store_dir(root.path(), removed.into_bytes()).unwrap();
        assert_eq!(
            list_store_dirs(root.path()).unwrap(),
            vec![kept.into_bytes()]
        );
        assert!(
            list_store_dirs(&root.path().join("missing"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn store_folders_are_private() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("webdata");
        let (fresh, earlier) = (Uuid::from_u128(1), Uuid::from_u128(2));
        std::fs::create_dir_all(store_dir(&root, earlier.into_bytes())).unwrap();
        std::fs::set_permissions(&root, Permissions::from_mode(0o755)).unwrap();
        for id in [fresh, earlier] {
            let dir = private_store_dir(&root, id.into_bytes()).unwrap();
            assert_eq!(dir, store_dir(&root, id.into_bytes()));
            for folder in [&root, &dir] {
                let mode = std::fs::metadata(folder).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o700, "{}", folder.display());
            }
        }
    }
}
