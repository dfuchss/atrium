//! Quitting and restarting after the service pages had a chance to unload.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use protocol::UI_WEBVIEW;
use tauri::{AppHandle, Manager, Runtime, Url};

use crate::Logger;
use crate::logger::spawn_logged;

/// Set once [`graceful_quit`] started; the exit it ends with must not start another one.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Time the service pages get for their `pagehide`/`unload` handlers before the process exits.
const UNLOAD_GRACE: Duration = Duration::from_millis(400);

/// Whether [`graceful_quit`] already ran (the app is exiting).
pub fn is_quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// Quits the app after unloading every service page (service views, popup and app windows).
///
/// Navigating to `about:blank` fires `pagehide`/`unload`, so pages release what they hold
/// (Element's session lock would otherwise look alive on the next start and show "Element is
/// open in another window"). The wait runs on a background thread, then `app.exit(0)`, whose
/// `ExitRequested` must be let through (see [`is_quitting`]). Only the first call (of this or
/// [`graceful_restart`]) does anything.
pub fn graceful_quit<R: Runtime>(app: &AppHandle<R>) {
    graceful_exit(app, false);
}

/// Like [`graceful_quit`], but starts the app again (`AppHandle::request_restart`).
pub fn graceful_restart<R: Runtime>(app: &AppHandle<R>) {
    graceful_exit(app, true);
}

fn graceful_exit<R: Runtime>(app: &AppHandle<R>, restart: bool) {
    if QUITTING.swap(true, Ordering::SeqCst) {
        return;
    }
    let logger = Logger::new(app);
    let unloaded = unload_service_pages(app, &logger);
    let grace = if unloaded == 0 {
        Duration::ZERO
    } else {
        UNLOAD_GRACE
    };
    let end = move |app: &AppHandle<R>| {
        if restart {
            app.request_restart();
        } else {
            app.exit(0);
        }
    };
    let exiting = app.clone();
    let spawned = spawn_logged("quit", &logger, move || {
        std::thread::sleep(grace);
        end(&exiting);
    });
    if !spawned {
        end(app);
    }
}

/// Navigates every webview except the sidebar to `about:blank`; returns their number.
fn unload_service_pages<R: Runtime>(app: &AppHandle<R>, logger: &Logger) -> usize {
    let views: Vec<_> = app
        .webviews()
        .into_values()
        .filter(|webview| webview.label() != UI_WEBVIEW)
        .collect();
    logger.log(&format!("quit: unloading {} service views", views.len()));
    let blank = Url::parse("about:blank").expect("valid URL");
    for view in &views {
        if let Err(error) = view.navigate(blank.clone()) {
            logger.log(&format!("quit: cannot unload {}: {error}", view.label()));
        }
    }
    views.len()
}
