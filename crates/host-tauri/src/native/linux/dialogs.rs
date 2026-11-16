//! JavaScript dialogs: Atrium's own, decided by the shared policy of `dialogs` (see
//! `webview_dialogs::linux`), for the service views and the windows they open.

use std::sync::Arc;

use tauri::{AppHandle, Runtime, Webview};
use webview_dialogs::DialogHandler;

use crate::Logger;

/// Makes `alert`, `confirm` and `prompt` of the webviews attached from now on show native
/// dialogs decided by `handler`.
pub(crate) fn install_dialogs<R: Runtime>(
    _app: &AppHandle<R>,
    handler: Arc<dyn DialogHandler>,
    logger: &Logger,
) {
    match webview_dialogs::install(handler) {
        Ok(()) => logger.log("JavaScript dialogs: Atrium's own"),
        Err(error) => logger.log(&format!("JavaScript dialogs unavailable: {error}")),
    }
}

/// Shows the dialogs of `webview` through the installed handler; they carry its label (see
/// `dialogs::account_of_label`).
pub(crate) fn attach_dialogs<R: Runtime>(webview: &Webview<R>, logger: &Logger) {
    let (label, attach_logger) = (webview.label().to_string(), logger.clone());
    let scheduled = webview.with_webview(move |platform| {
        if let Err(error) = webview_dialogs::attach(&platform.inner(), &label) {
            attach_logger.log(&format!("[{label}] cannot attach dialogs: {error}"));
        }
    });
    if let Err(error) = scheduled {
        logger.log(&format!(
            "[{}] cannot attach dialogs: {error}",
            webview.label()
        ));
    }
}
