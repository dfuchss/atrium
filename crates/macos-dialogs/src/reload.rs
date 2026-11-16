//! Reload of a webview that bypasses the cache.

use objc2_web_kit::WKWebView;
use tauri::webview::PlatformWebview;

/// Reloads the page from its origin: `WebKit` revalidates every resource with the server
/// instead of using the cache (Safari's "Reload Page From Origin"). Call it through
/// `Webview::with_webview`, which runs on the main thread.
pub fn reload_from_origin(webview: &PlatformWebview) {
    let inner: *const WKWebView = webview.inner().cast();
    // SAFETY: `inner()` is the `WKWebView` Tauri built for this webview, alive while `webview`
    // is borrowed, and `with_webview` calls this on the main thread `WKWebView` requires.
    // `reloadFromOrigin` takes no arguments; its navigation (None without a page) is not needed.
    let _navigation = unsafe { (*inner).reloadFromOrigin() };
}
