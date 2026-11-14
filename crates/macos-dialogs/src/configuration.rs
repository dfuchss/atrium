//! `WebKit` configuration of service webviews: their pages open windows only from a user
//! gesture.
//!
//! `WKPreferences.javaScriptCanOpenWindowsAutomatically` is YES on macOS and wry never changes
//! it, so a page could call `window.open` from a timer. The preference is read when a webview is
//! created from the configuration, so it is set on the configuration, before the page runs.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_foundation::NSUUID;
use objc2_web_kit::{WKWebViewConfiguration, WKWebsiteDataStore};

/// A configuration for a service webview: the persistent data store `store_id` (what wry sets
/// up itself when it creates the configuration; with a given one it takes the store from it)
/// and no windows without a user gesture.
pub fn service_configuration(
    mtm: MainThreadMarker,
    store_id: [u8; 16],
) -> Retained<WKWebViewConfiguration> {
    // SAFETY: a plain `[[WKWebViewConfiguration alloc] init]` on the main thread (`mtm`).
    let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
    let identifier = NSUUID::from_bytes(store_id);
    // SAFETY: `dataStoreForIdentifier:` (macOS 14+, Atrium's minimum) takes any UUID and returns
    // the persistent store for it, created on first use; called on the main thread (`mtm`).
    let data_store = unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&identifier, mtm) };
    // SAFETY: setting the data store of a configuration no webview uses yet.
    unsafe { configuration.setWebsiteDataStore(&data_store) };
    forbid_automatic_windows(&configuration);
    configuration
}

/// Makes `window.open` need a user gesture in webviews created from `configuration` (e.g. the
/// one `WebKit` hands over for a popup, which starts as a copy of the opener's).
pub fn forbid_automatic_windows(configuration: &WKWebViewConfiguration) {
    // SAFETY: `preferences` is never nil for a configuration, and the setter takes a plain
    // bool; `WKWebViewConfiguration` is main-thread-only, so this runs on the main thread.
    unsafe {
        configuration
            .preferences()
            .setJavaScriptCanOpenWindowsAutomatically(false);
    }
}
