//! The calls to the system webviews and the OS that Tauri has no API for. Foremost the
//! JavaScript dialogs (`alert`, `confirm`, `prompt`): the platform-neutral request and answer
//! types, decided by a [`DialogHandler`], and the glue to the engine of each platform:
//! `WKWebView` on macOS (`macos`), `WebKitGTK` on Linux (`linux`), `WebView2` on Windows
//! (`windows`). On macOS also the other `WebKit` calls: the service view configuration, reloads
//! from origin, display wakes, the audio mute and the capture state; on Linux too the audio mute
//! and the capture state; on Windows the other `WebView2` and Win32 calls (the audio mute
//! among them), and the shell shortcuts of the Start menu (`shortcut`).
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

#[cfg(windows)]
mod shortcut;

#[cfg(windows)]
pub use shortcut::{Shortcut, read_shortcut, write_shortcut};

/// Why the dialogs could not be installed or attached. Nothing changes on an error: the pages
/// keep getting the default answers.
#[derive(Debug)]
pub enum InstallError {
    AlreadyInstalled,
    /// A webview was attached before `install`.
    #[cfg(any(target_os = "linux", windows))]
    NotInstalled,
    /// The webview given to `install` has no UI delegate.
    #[cfg(target_os = "macos")]
    NoDelegate,
    #[cfg(target_os = "macos")]
    DonorClassExists,
    #[cfg(target_os = "macos")]
    MissingDonorMethod(String),
    /// wry implements the method itself; its own handling is kept.
    #[cfg(target_os = "macos")]
    AlreadyImplemented(String),
    #[cfg(windows)]
    WebView2(::windows::core::Error),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::AlreadyInstalled => write!(formatter, "dialogs are already installed"),
            #[cfg(any(target_os = "linux", windows))]
            InstallError::NotInstalled => write!(formatter, "dialogs are not installed"),
            #[cfg(target_os = "macos")]
            InstallError::NoDelegate => write!(formatter, "the webview has no UI delegate"),
            #[cfg(target_os = "macos")]
            InstallError::DonorClassExists => write!(
                formatter,
                "class {} exists already",
                macos::DONOR_CLASS.to_string_lossy()
            ),
            #[cfg(target_os = "macos")]
            InstallError::MissingDonorMethod(selector) => {
                write!(formatter, "donor class lacks {selector}")
            }
            #[cfg(target_os = "macos")]
            InstallError::AlreadyImplemented(selector) => {
                write!(formatter, "wry implements {selector} itself")
            }
            #[cfg(windows)]
            InstallError::WebView2(error) => write!(formatter, "WebView2: {error}"),
        }
    }
}

impl std::error::Error for InstallError {}

#[cfg(windows)]
impl From<::windows::core::Error> for InstallError {
    fn from(error: ::windows::core::Error) -> InstallError {
        InstallError::WebView2(error)
    }
}

/// What `install` sets once for the whole process: the [`DialogHandler`], with whatever else
/// the platform needs.
struct HandlerSlot<T>(std::sync::OnceLock<T>);

impl<T> HandlerSlot<T> {
    const fn new() -> HandlerSlot<T> {
        HandlerSlot(std::sync::OnceLock::new())
    }

    fn install(&self, installed: T) -> Result<(), InstallError> {
        self.0
            .set(installed)
            .map_err(|_| InstallError::AlreadyInstalled)
    }

    fn get(&self) -> Option<&T> {
        self.0.get()
    }

    #[cfg(any(target_os = "linux", windows))]
    fn installed(&self) -> Result<&T, InstallError> {
        self.get().ok_or(InstallError::NotInstalled)
    }
}

/// Answers the engine's dialog exactly once: through the function [`take`](Self::take) hands
/// out, or with [`DialogAnswer::Cancelled`] (the default) when dropped untaken, as when the
/// panel goes with its window before the user answered.
#[cfg(any(target_os = "macos", target_os = "linux"))]
struct AnswerOnce<F: FnOnce(DialogAnswer)> {
    finish: std::cell::RefCell<Option<F>>,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl<F: FnOnce(DialogAnswer)> AnswerOnce<F> {
    fn new(finish: F) -> AnswerOnce<F> {
        AnswerOnce {
            finish: std::cell::RefCell::new(Some(finish)),
        }
    }

    /// The function answering the dialog; None once taken.
    fn take(&self) -> Option<F> {
        self.finish.borrow_mut().take()
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl<F: FnOnce(DialogAnswer)> Drop for AnswerOnce<F> {
    fn drop(&mut self) {
        if let Some(finish) = self.finish.get_mut().take() {
            finish(DialogAnswer::Cancelled);
        }
    }
}

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

impl DialogAnswer {
    /// The answer of a panel closed with OK (`ok`) or otherwise (Cancel, Escape, closing it):
    /// OK answers with the `entered` text of a prompt, or as confirmed without a text field;
    /// anything else cancels.
    pub fn from_ok(ok: bool, entered: Option<String>) -> DialogAnswer {
        match (ok, entered) {
            (true, Some(text)) => DialogAnswer::Text(text),
            (true, None) => DialogAnswer::Confirmed,
            (false, _) => DialogAnswer::Cancelled,
        }
    }
}

/// Decides and observes the dialogs; called on the main thread, so it must not block.
pub trait DialogHandler: Send + Sync {
    fn present(&self, request: &DialogRequest) -> Presentation;
    fn answered(&self, request: &DialogRequest, answer: &DialogAnswer, suppress_further: bool);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_answers_with_the_text_or_confirms() {
        assert_eq!(
            DialogAnswer::from_ok(true, Some("typed".into())),
            DialogAnswer::Text("typed".into())
        );
        assert_eq!(DialogAnswer::from_ok(true, None), DialogAnswer::Confirmed);
    }

    #[test]
    fn everything_but_ok_cancels() {
        assert_eq!(
            DialogAnswer::from_ok(false, Some("typed".into())),
            DialogAnswer::Cancelled
        );
        assert_eq!(DialogAnswer::from_ok(false, None), DialogAnswer::Cancelled);
    }

    #[test]
    fn a_handler_slot_is_set_once() {
        let slot = HandlerSlot::new();
        assert!(slot.get().is_none());
        assert!(slot.install(1).is_ok());
        assert!(matches!(
            slot.install(2),
            Err(InstallError::AlreadyInstalled)
        ));
        assert_eq!(slot.get(), Some(&1));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn recorded_answers() -> (
        std::rc::Rc<std::cell::RefCell<Vec<DialogAnswer>>>,
        impl FnOnce(DialogAnswer),
    ) {
        let answers = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = answers.clone();
        (answers, move |answer| recorded.borrow_mut().push(answer))
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_dialog_is_answered_once() {
        let (answers, finish) = recorded_answers();
        let once = AnswerOnce::new(finish);
        if let Some(finish) = once.take() {
            finish(DialogAnswer::Confirmed);
        }
        assert!(once.take().is_none());
        drop(once);
        assert_eq!(*answers.borrow(), [DialogAnswer::Confirmed]);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_dialog_dropped_unanswered_is_cancelled() {
        let (answers, finish) = recorded_answers();
        drop(AnswerOnce::new(finish));
        assert_eq!(*answers.borrow(), [DialogAnswer::Cancelled]);
    }

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn origin_is_host_and_non_default_port() {
        let origin = origin_of("https://chat.example.org/room/1");
        assert_eq!(origin.host, "chat.example.org");
        assert_eq!(origin.port, 0);
        assert_eq!(origin_of("http://127.0.0.1:8765/").port, 8765);
        assert_eq!(origin_of("https://example.org:443/").port, 0);
    }

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn pages_without_host_have_an_empty_origin() {
        for uri in ["about:blank", "data:text/html,hi", "not a uri"] {
            assert_eq!(origin_of(uri).host, "", "{uri}");
        }
    }
}
