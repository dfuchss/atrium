//! Child webviews that Tauri lays out by itself (all but Linux, where GTK needs a layout of
//! Atrium's): added at their bounds, moved, resized, shown and hidden through Tauri.

use domain::Bounds;
use domain::ports::PortResult;
use tauri::{LogicalPosition, LogicalSize, Runtime, Webview, WebviewBuilder, Window};

use crate::port_err;

pub(crate) fn add_child_at<R: Runtime>(
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
