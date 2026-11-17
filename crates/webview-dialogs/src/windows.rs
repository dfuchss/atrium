//! Windows (`WebView2`): Atrium's own `alert`, `confirm` and `prompt` for pages in service
//! webviews and the windows they open, the reload past the cache and the warning box, and in a
//! module each the key presses of a webview (`keys`), its failed processes (`process`), the
//! resumes from sleep (`power`) and the app's `AppUserModelID` (`app_id`): the `WebView2` and
//! Win32 calls Tauri has no API for.
//!
//! [`attach`] turns `WebView2`'s own dialogs of a webview off and handles its
//! `ScriptDialogOpening` event: what a dialog shows is decided by the [`DialogHandler`] (text,
//! suppression, rate limits), a dialog to show goes to the [`ShowDialog`] callback given to
//! [`install`], which shows it in a window of Atrium's own. The page waits on the event's
//! deferral until [`answer`] completes it. Before-unload prompts are confirmed without a
//! dialog, as `WebKit` does on macOS, so a page can neither keep itself open nor block quitting.
//!
//! The origin is that of the URI `WebView2` reports for the asking page, so a page cannot pose
//! as another. `WebView2` raises the event on the main thread, whose objects stay there: the
//! dialogs waiting for their answer are kept per thread, and [`answer`] must run there too.

mod app_id;
mod keys;
mod power;
mod process;

pub use app_id::*;
pub use keys::*;
pub use power::*;
pub use process::*;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_SCRIPT_DIALOG_KIND, COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT,
    COREWEBVIEW2_SCRIPT_DIALOG_KIND_CONFIRM, COREWEBVIEW2_SCRIPT_DIALOG_KIND_PROMPT,
    ICoreWebView2Controller, ICoreWebView2Deferral, ICoreWebView2ScriptDialogOpeningEventArgs,
};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, ScriptDialogOpeningEventHandler, take_pwstr,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_TOP, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TASKMODAL, MessageBoxW,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, ShowWindow,
};
use windows::core::{BOOL, HSTRING, PWSTR};

use crate::{
    DialogAnswer, DialogHandler, DialogKind, DialogRequest, DialogText, Presentation, origin_of,
};

#[derive(Debug)]
pub enum InstallError {
    AlreadyInstalled,
    /// [`attach`] was called before [`install`].
    NotInstalled,
    /// `WebView2` refused a call.
    WebView2(windows::core::Error),
}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::AlreadyInstalled => write!(formatter, "dialogs are already installed"),
            InstallError::NotInstalled => write!(formatter, "dialogs are not installed"),
            InstallError::WebView2(error) => write!(formatter, "WebView2: {error}"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<windows::core::Error> for InstallError {
    fn from(error: windows::core::Error) -> InstallError {
        InstallError::WebView2(error)
    }
}

/// A dialog the handler decided to show; whatever shows it answers through [`answer`].
#[derive(Debug, Clone)]
pub struct ShownDialog {
    /// Names the dialog to [`answer`].
    pub id: u64,
    pub request: DialogRequest,
    pub text: DialogText,
    /// The asking webview is visible. A dialog of a hidden one (an account not on screen)
    /// should take no keys meant for the visible account.
    pub takes_keys: bool,
}

/// Shows a dialog. Called on the main thread from inside `WebView2`'s event, so it must
/// neither block nor create webviews there.
pub type ShowDialog = Box<dyn Fn(ShownDialog) + Send + Sync>;

struct Installed {
    handler: Arc<dyn DialogHandler>,
    show: ShowDialog,
}

static INSTALLED: OnceLock<Installed> = OnceLock::new();

/// A shown dialog's request with `WebView2`'s event, whose deferral keeps the page waiting.
struct Pending {
    request: DialogRequest,
    event: ICoreWebView2ScriptDialogOpeningEventArgs,
    deferral: ICoreWebView2Deferral,
}

thread_local! {
    /// The shown dialogs by id, on the main thread.
    static PENDING: RefCell<HashMap<u64, Pending>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

/// Sets the handler deciding the dialogs of every webview given to [`attach`] later, and how a
/// dialog to show is shown.
pub fn install(handler: Arc<dyn DialogHandler>, show: ShowDialog) -> Result<(), InstallError> {
    INSTALLED
        .set(Installed { handler, show })
        .map_err(|_| InstallError::AlreadyInstalled)
}

/// Shows the dialogs of the pages in `controller`'s webview as decided by the installed
/// handler; their [`DialogRequest`]s carry `tag`. Call it on the main thread.
pub fn attach(controller: &ICoreWebView2Controller, tag: &str) -> Result<(), InstallError> {
    let installed = INSTALLED.get().ok_or(InstallError::NotInstalled)?;
    // The handler holds the controller to ask whether the view is visible; wry closes the
    // controller when the webview goes, which ends the view either way.
    let (asking, tag) = (controller.clone(), tag.to_string());
    let handler = ScriptDialogOpeningEventHandler::create(Box::new(move |_, event| match event {
        Some(event) => on_script_dialog(installed, &asking, &event, &tag),
        None => Ok(()),
    }));
    let mut token = 0;
    // SAFETY: plain COM calls on the main thread, which owns the controller; the handler is a
    // valid COM object and the token a valid out pointer for the duration of the call.
    unsafe {
        let webview = controller.CoreWebView2()?;
        webview
            .Settings()?
            .SetAreDefaultScriptDialogsEnabled(false)?;
        webview.add_ScriptDialogOpening(&handler, &raw mut token)?;
    }
    Ok(())
}

/// Handles one `ScriptDialogOpening` event. Without `Accept` the page gets the default answer
/// (`confirm` -> false, `prompt` -> null).
fn on_script_dialog(
    installed: &Installed,
    controller: &ICoreWebView2Controller,
    event: &ICoreWebView2ScriptDialogOpeningEventArgs,
    tag: &str,
) -> windows::core::Result<()> {
    let mut kind = COREWEBVIEW2_SCRIPT_DIALOG_KIND::default();
    // SAFETY: a COM getter of the event on the main thread, with a valid out pointer.
    unsafe { event.Kind(&raw mut kind) }?;
    let Some(kind) = kind_of(kind, || {
        // SAFETY: as above; `read_string` frees the returned string.
        read_string(|text| unsafe { event.DefaultText(text) }).unwrap_or_default()
    }) else {
        // SAFETY: a COM call of the event on the main thread, before the handler returns.
        return unsafe { event.Accept() };
    };
    // SAFETY: COM getters of the event on the main thread; `read_string` frees the strings.
    let message = read_string(|text| unsafe { event.Message(text) })?;
    // SAFETY: as above.
    let uri = read_string(|uri| unsafe { event.Uri(uri) })?;
    let request = DialogRequest {
        kind,
        message,
        origin: origin_of(&uri),
        webview_tag: Some(tag.to_string()),
    };
    match installed.handler.present(&request) {
        Presentation::Suppress => Ok(()),
        Presentation::Show(text) => {
            let shown = wait_for_answer(event, request, text, is_visible(controller))?;
            (installed.show)(shown);
            Ok(())
        }
    }
}

/// The dialog kind of a `WebView2` dialog kind; None for before-unload prompts. `default_text`
/// is only asked for prompts.
fn kind_of(
    kind: COREWEBVIEW2_SCRIPT_DIALOG_KIND,
    default_text: impl FnOnce() -> String,
) -> Option<DialogKind> {
    match kind {
        COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT => Some(DialogKind::Alert),
        COREWEBVIEW2_SCRIPT_DIALOG_KIND_CONFIRM => Some(DialogKind::Confirm),
        COREWEBVIEW2_SCRIPT_DIALOG_KIND_PROMPT => Some(DialogKind::Prompt {
            default_text: default_text(),
        }),
        _ => None,
    }
}

/// A string `WebView2` hands out through `get`, freed once copied.
fn read_string(
    get: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>,
) -> windows::core::Result<String> {
    let mut text = PWSTR::null();
    get(&raw mut text)?;
    Ok(take_pwstr(text))
}

/// Whether the webview of `controller` is shown; a failed query counts as hidden.
fn is_visible(controller: &ICoreWebView2Controller) -> bool {
    let mut visible = BOOL::default();
    // SAFETY: a COM getter on the main thread, with a valid out pointer.
    let queried = unsafe { controller.IsVisible(&raw mut visible) };
    queried.is_ok() && visible.as_bool()
}

/// Keeps the page of `event` waiting for the answer to `request` and names the dialog.
fn wait_for_answer(
    event: &ICoreWebView2ScriptDialogOpeningEventArgs,
    request: DialogRequest,
    text: DialogText,
    takes_keys: bool,
) -> windows::core::Result<ShownDialog> {
    // SAFETY: a COM call of the event on the main thread, before the handler returns.
    let deferral = unsafe { event.GetDeferral() }?;
    let id = NEXT_ID.replace(NEXT_ID.get() + 1);
    let pending = Pending {
        request: request.clone(),
        event: event.clone(),
        deferral,
    };
    PENDING.with_borrow_mut(|dialogs| dialogs.insert(id, pending));
    Ok(ShownDialog {
        id,
        request,
        text,
        takes_keys,
    })
}

/// Answers the shown dialog `id` and resumes its page. False if it does not wait for an answer
/// (answered already, or never shown). Call it on the main thread.
pub fn answer(id: u64, answer: &DialogAnswer, suppress_further: bool) -> bool {
    let Some(pending) = PENDING.with_borrow_mut(|dialogs| dialogs.remove(&id)) else {
        return false;
    };
    if let Some(installed) = INSTALLED.get() {
        installed
            .handler
            .answered(&pending.request, answer, suppress_further);
    }
    // Fails only when the page or its webview is gone, which then waits for nothing.
    let _ = pending.finish(answer);
    true
}

impl Pending {
    /// Sets `answer` on the event (without `Accept`, the page gets the default answer) and
    /// completes the deferral, which resumes the page.
    fn finish(&self, answer: &DialogAnswer) -> windows::core::Result<()> {
        // SAFETY: COM calls of the event and its deferral on the main thread, which owns them.
        unsafe {
            match answer {
                DialogAnswer::Cancelled => {}
                DialogAnswer::Confirmed => self.event.Accept()?,
                DialogAnswer::Text(text) => {
                    self.event.SetResultText(&HSTRING::from(text))?;
                    self.event.Accept()?;
                }
            }
            self.deferral.Complete()
        }
    }
}

/// Reloads the page of `controller`'s webview past the cache (the `DevTools` protocol's
/// `Page.reload` with `ignoreCache`, as a browser's hard reload) without waiting for it; a
/// failure on the way is handed to `on_error`. Call it on the main thread.
pub fn reload_ignoring_cache(
    controller: &ICoreWebView2Controller,
    on_error: impl FnOnce(String) + 'static,
) -> windows::core::Result<()> {
    let completed =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _response| {
            if let Err(error) = result {
                on_error(error.to_string());
            }
            Ok(())
        }));
    // SAFETY: plain COM calls on the main thread, which owns the controller; the strings are
    // valid, null-terminated UTF-16 for the duration of the call, the handler a valid COM object.
    unsafe {
        controller.CoreWebView2()?.CallDevToolsProtocolMethod(
            &HSTRING::from("Page.reload"),
            &HSTRING::from(r#"{"ignoreCache":true}"#),
            &completed,
        )
    }
}

/// Shows `window` above its owner without activating it, so the window that has the keyboard
/// keeps it (tao's `show` always activates). tao still counts the window as hidden: a later
/// change of its window flags (resizable, maximized, ...) would hide it again.
pub fn show_without_activating(window: HWND) {
    // SAFETY: both calls take any window handle and fail harmlessly for an invalid one; the
    // flags keep position, size and activation as they are.
    unsafe {
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            window,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        );
    }
}

/// Shows a warning with one OK button, modal for the windows of the calling thread, and returns
/// once it is dismissed. It pumps messages meanwhile, so call it from the running event loop.
pub fn show_warning(title: &str, message: &str) {
    let text = HSTRING::from(format!("{title}\n\n{message}"));
    // SAFETY: both strings are valid, null-terminated UTF-16 that outlives the call, no owner
    // window is passed, and the style is a valid combination of flags.
    unsafe {
        MessageBoxW(
            None,
            &text,
            &HSTRING::from("Atrium"),
            MB_OK | MB_ICONWARNING | MB_TASKMODAL | MB_SETFOREGROUND,
        );
    }
}

#[cfg(test)]
mod tests {
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD;

    use super::*;

    #[test]
    fn kinds_follow_the_webview2_kinds() {
        assert_eq!(
            kind_of(COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT, || unreachable!()),
            Some(DialogKind::Alert)
        );
        assert_eq!(
            kind_of(COREWEBVIEW2_SCRIPT_DIALOG_KIND_CONFIRM, || unreachable!()),
            Some(DialogKind::Confirm)
        );
        assert_eq!(
            kind_of(COREWEBVIEW2_SCRIPT_DIALOG_KIND_PROMPT, || "default".into()),
            Some(DialogKind::Prompt {
                default_text: "default".into()
            })
        );
        assert_eq!(
            kind_of(COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD, || {
                unreachable!()
            }),
            None
        );
    }

    #[test]
    fn unknown_dialogs_wait_for_no_answer() {
        assert!(!answer(u64::MAX, &DialogAnswer::Confirmed, false));
    }
}
