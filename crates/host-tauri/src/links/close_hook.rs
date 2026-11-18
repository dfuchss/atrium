//! Popups closing themselves: wry does not close a window on its page's `window.close()`.

use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};

use crate::Logger;
use crate::logger::spawn_logged;

/// wry cannot close a popup on its own `window.close()` (end of an OAuth flow): on macOS it does
/// not implement `WebKit`'s `webViewDidClose`, on Linux it destroys the webview but leaves its
/// window (and a service view with a single page in its history may close itself). So this
/// wrapper only records the call; [`watch_close`] polls it in popups and closes their window.
/// Nothing polls a service view, where the call does nothing.
pub(crate) const CLOSE_HOOK: &str = "(()=>{if(window.__atriumCloseHook)return;window.__atriumCloseHook=1;window.close=function(){window.__atriumClosed=true;};})();";

/// Closes the window `label` once its page called `window.close()` (see [`CLOSE_HOOK`]);
/// ends when the window is gone.
pub(super) fn watch_close<R: Runtime>(app: AppHandle<R>, label: String, logger: &Logger) {
    spawn_logged(&format!("watch-{label}"), logger, move || {
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let Some(window) = app.get_webview_window(&label) else {
                break;
            };
            let target = window.clone();
            let _ = window.eval_with_callback("!!window.__atriumClosed", move |raw| {
                if raw == "true" {
                    let _ = target.close();
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_hook_only_records_the_call() {
        assert!(CLOSE_HOOK.contains("window.close=function(){window.__atriumClosed=true;}"));
        assert!(!CLOSE_HOOK.contains(".close("));
    }
}
