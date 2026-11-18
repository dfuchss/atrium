//! JavaScript dialogs: Atrium's own, decided by the shared policy of `dialogs` (see
//! `platform_ffi::linux`), for the service views and the windows they open.

use std::sync::Arc;

use platform_ffi::DialogHandler;
use tauri::{AppHandle, Runtime, Webview};

use crate::Logger;
use crate::native::with_platform_webview;

/// Makes `alert`, `confirm` and `prompt` of the webviews attached from now on show native
/// dialogs decided by `handler`.
pub(crate) fn install_dialogs<R: Runtime>(
    _app: &AppHandle<R>,
    handler: Arc<dyn DialogHandler>,
    logger: &Logger,
) {
    match platform_ffi::install(handler) {
        Ok(()) => logger.log("JavaScript dialogs: Atrium's own"),
        Err(error) => logger.log(&format!("JavaScript dialogs unavailable: {error}")),
    }
}

/// Shows the dialogs of `webview` through the installed handler; they carry its label (see
/// `labels::account_of_label`).
pub(crate) fn attach_dialogs<R: Runtime>(webview: &Webview<R>, logger: &Logger) {
    let label = webview.label().to_string();
    with_platform_webview(webview, logger, "cannot attach dialogs", move |platform| {
        platform_ffi::attach(&platform.inner(), &label)
    });
}
