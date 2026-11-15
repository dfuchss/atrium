//! Linux: Tauri packs the child webviews of a window into its `GtkBox`, one below the other,
//! and ignores their bounds. [`adopt`] moves a child into a `GtkLayout` in that box, where
//! [`place`] positions it; unlike a `GtkFixed`, the layout's size does not follow its children,
//! so the window can still shrink. The layout is the area below the menu bar, whose size
//! [`track_content_size`] reports.

use std::cell::Cell;

use domain::Bounds;
use gtk::prelude::*;
use tauri::{Runtime, Webview, Window};

/// Reports the logical size of the area the child webviews of `window` share whenever it
/// changes: the window's inner size includes the menu bar. Call on the main thread.
pub(crate) fn track_content_size<R: Runtime>(
    window: &Window<R>,
    on_resize: impl Fn(f64, f64) + 'static,
) -> tauri::Result<()> {
    let layout = layout_in(&window.default_vbox()?);
    let last = Cell::new((0, 0));
    // Every layout pass allocates, also when placing a child changed nothing.
    layout.connect_size_allocate(move |_, allocation| {
        let size = (allocation.width(), allocation.height());
        if last.replace(size) != size {
            on_resize(f64::from(size.0), f64::from(size.1));
        }
    });
    Ok(())
}

/// Moves `webview` from the window's box into the box's layout (created on first use) at
/// `bounds`. Runs on the main thread like every later [`place`], so they keep their order.
pub(crate) fn adopt<R: Runtime>(webview: &Webview<R>, bounds: Bounds) -> tauri::Result<()> {
    webview.with_webview(move |platform| {
        let widget = platform.inner();
        let Some(container) = widget
            .parent()
            .and_then(|parent| parent.downcast::<gtk::Box>().ok())
        else {
            return;
        };
        let layout = layout_in(&container);
        container.remove(&widget);
        let (x, y, width, height) = pixels(bounds);
        layout.put(&widget, x, y);
        widget.set_size_request(width, height);
    })
}

/// Moves and sizes an adopted `webview`.
pub(crate) fn place<R: Runtime>(webview: &Webview<R>, bounds: Bounds) -> tauri::Result<()> {
    webview.with_webview(move |platform| {
        let widget = platform.inner();
        let Some(layout) = widget
            .parent()
            .and_then(|parent| parent.downcast::<gtk::Layout>().ok())
        else {
            return;
        };
        let (x, y, width, height) = pixels(bounds);
        layout.move_(&widget, x, y);
        widget.set_size_request(width, height);
    })
}

fn layout_in(container: &gtk::Box) -> gtk::Layout {
    if let Some(layout) = container
        .children()
        .into_iter()
        .find_map(|child| child.downcast::<gtk::Layout>().ok())
    {
        return layout;
    }
    let layout = gtk::Layout::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    container.pack_start(&layout, true, true, 0);
    layout.show();
    layout
}

/// Logical bounds in whole GTK (logical) pixels.
#[expect(
    clippy::cast_possible_truncation,
    reason = "window coordinates are far inside the i32 range"
)]
fn pixels(bounds: Bounds) -> (i32, i32, i32, i32) {
    (
        bounds.x.round() as i32,
        bounds.y.round() as i32,
        bounds.width.round() as i32,
        bounds.height.round() as i32,
    )
}
