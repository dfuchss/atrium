//! JavaScript dialogs (`alert`, `confirm`, `prompt`) of the system webviews: the
//! platform-neutral request and answer types, decided by a [`DialogHandler`], and the glue to
//! the engine of each platform: `WKWebView` on macOS (`macos`), `WebKitGTK` on Linux (`linux`),
//! `WebView2` on Windows (`windows`). On macOS it also holds the other `WebKit` calls Tauri has
//! no API for: the service view configuration, reloads from origin and display wakes; on
//! Windows the `WebView2` and Win32 calls Tauri has no API for.
//!
//! The macOS and Windows glue holds the `unsafe` code of this crate; every block says why it is
//! sound. Besides it, only the Linux theme of `host-tauri` has `unsafe` code.

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::*;

/// Host and port of the page at `uri`; an empty host for pages without one (`about:blank`,
/// `data:`).
#[cfg(any(target_os = "linux", windows))]
fn origin_of(uri: &str) -> PageOrigin {
    let url = url::Url::parse(uri).ok();
    PageOrigin {
        host: url
            .as_ref()
            .and_then(url::Url::host_str)
            .unwrap_or_default()
            .to_string(),
        port: url.as_ref().and_then(url::Url::port).unwrap_or(0),
    }
}

/// Which panel the page asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogKind {
    Alert,
    Confirm,
    Prompt { default_text: String },
}

/// Origin of the frame that asked, as `WebKit` reports it (the page cannot fake it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageOrigin {
    /// Empty for pages without a host (`about:blank`, `data:`).
    pub host: String,
    /// 0 = the scheme's default port.
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogRequest {
    pub kind: DialogKind,
    pub message: String,
    pub origin: PageOrigin,
    /// The tag the host attached to the asking webview, if any.
    pub webview_tag: Option<String>,
}

/// What the panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogText {
    pub title: String,
    pub message: String,
    /// Add the "Prevent this page from creating additional dialogs" checkbox.
    pub offer_suppression: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presentation {
    Show(DialogText),
    /// Answer with the default (OK for `alert`, false for `confirm`, null for `prompt`).
    Suppress,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogAnswer {
    Cancelled,
    Confirmed,
    /// The text entered into a `prompt`.
    Text(String),
}

/// Decides and observes the dialogs; called on the main thread, so it must not block.
pub trait DialogHandler: Send + Sync {
    fn present(&self, request: &DialogRequest) -> Presentation;
    fn answered(&self, request: &DialogRequest, answer: &DialogAnswer, suppress_further: bool);
}

#[cfg(all(test, any(target_os = "linux", windows)))]
mod tests {
    use super::*;

    #[test]
    fn origin_is_host_and_non_default_port() {
        let origin = origin_of("https://chat.example.org/room/1");
        assert_eq!(origin.host, "chat.example.org");
        assert_eq!(origin.port, 0);
        assert_eq!(origin_of("http://127.0.0.1:8765/").port, 8765);
        assert_eq!(origin_of("https://example.org:443/").port, 0);
    }

    #[test]
    fn pages_without_host_have_an_empty_origin() {
        for uri in ["about:blank", "data:text/html,hi", "not a uri"] {
            assert_eq!(origin_of(uri).host, "", "{uri}");
        }
    }
}
