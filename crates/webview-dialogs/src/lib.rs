//! JavaScript dialogs (`alert`, `confirm`, `prompt`) of the system webviews: the
//! platform-neutral request and answer types, decided by a [`DialogHandler`], and the glue to
//! the engine of each platform. On macOS (`macos`) it also holds the other `WebKit` calls Tauri
//! has no API for: the service view configuration, reloads from origin and display wakes.
//!
//! The only crate of the workspace with `unsafe` code (the macOS glue); every block says why it
//! is sound.

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::*;

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
