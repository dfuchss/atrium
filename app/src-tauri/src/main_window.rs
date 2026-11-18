//! The main window: its creation, the docked sidebar and the window events it feeds the core.

use tauri::window::WindowBuilder;
use tauri::{App, LogicalSize, Manager, Theme, Window, WindowEvent};

use domain::Input;
use host_tauri::Feed;
use protocol::AppSettings;

use crate::dev_hooks::DevHooks;
use crate::snapshot_cache::SnapshotCache;

pub(crate) const MAIN_WINDOW: &str = "main";

/// Shows, unminimizes and focuses the main window (dock icon click while hidden).
#[cfg(target_os = "macos")]
pub(crate) fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_window(MAIN_WINDOW) {
        // Nothing sensible to do if the window cannot come to the front.
        let _ = host_tauri::bring_to_front(&window);
    }
}

/// On macOS a transparent title bar without a title: it shows the window background (the
/// sidebar color) instead of a lighter strip. Not Overlay: the content view stays below the
/// title bar, so the services' top-edge controls keep working.
fn build_main_window(app: &App, dev: &DevHooks, start_hidden: bool) -> tauri::Result<Window> {
    let builder = WindowBuilder::new(app, MAIN_WINDOW).title("Atrium");
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Transparent)
        .hidden_title(true);
    let mut builder = builder
        .inner_size(1280.0, 800.0)
        .min_inner_size(600.0, 400.0)
        .visible(!start_hidden)
        .always_on_top(dev.always_on_top);
    if let Some((x, y)) = dev.position {
        builder = builder.position(x, y);
    }
    let window = builder.build()?;
    window.set_background_color(Some(host_tauri::window_background(
        window.theme().unwrap_or(Theme::Light),
    )))?;
    Ok(window)
}

fn feed_resize(feed: &Feed, size: LogicalSize<f64>) {
    feed(Input::WindowResized {
        width: size.width,
        height: size.height,
    });
}

/// Closing the window (red button, Cmd+W) hides it or quits, as the settings say; Quit
/// (Cmd+Q) is a menu item (see `host_tauri::graceful_quit`).
fn close_requested(window: &Window, snapshot_cache: &SnapshotCache) {
    if snapshot_cache.get().settings.close_to_background {
        let _ = window.hide();
    } else {
        // Keep the window (and its service views) until the pages are unloaded.
        host_tauri::graceful_quit(window.app_handle());
    }
}

/// Window events of the main window only: popup and app windows opened by services are normal
/// windows that really close.
pub(crate) fn watch_main_window(window: &Window, feed: Feed, snapshot_cache: SnapshotCache) {
    let event_window = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            close_requested(&event_window, &snapshot_cache);
        }
        WindowEvent::ThemeChanged(theme) => {
            let _ = event_window.set_background_color(Some(host_tauri::window_background(*theme)));
        }
        WindowEvent::Resized(size) => {
            // A Resized event reports the new physical size; the scale factor is unchanged
            // (ScaleFactorChanged carries its own).
            if let Ok(scale) = event_window.scale_factor() {
                feed_resize(&feed, size.to_logical::<f64>(scale));
            }
        }
        WindowEvent::ScaleFactorChanged {
            scale_factor,
            new_inner_size,
            ..
        } => feed_resize(&feed, new_inner_size.to_logical::<f64>(*scale_factor)),
        _ => {}
    });
}

/// Creates the main window, docks the sidebar webview at its full height and returns the
/// window with its logical size.
pub(crate) fn build_window_with_sidebar(
    app: &App,
    dev: &DevHooks,
    app_settings: &AppSettings,
) -> tauri::Result<(Window, LogicalSize<f64>)> {
    let window = build_main_window(app, dev, app_settings.start_hidden)?;
    let size = window
        .inner_size()?
        .to_logical::<f64>(window.scale_factor()?);
    host_tauri::add_sidebar(
        &window,
        dev.ui_url(),
        app_settings.icon_size.sidebar_width(),
        size.height,
    )?;
    Ok((window, size))
}
