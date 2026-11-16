//! macOS (`WKWebView`): native `alert`, `confirm` and `prompt` for pages in service webviews
//! and the windows they open.
//!
//! wry's `WKUIDelegate` class implements none of `WebKit`'s three JavaScript panel methods, so
//! `WebKit` answers them itself with the defaults (`confirm` -> false). [`install`] adds the
//! methods to that class at runtime (`class_addMethod`, the way an Objective-C category extends
//! a class): everything the delegate already does (new windows, media permission, file upload)
//! stays untouched, and should a wry upgrade implement the methods itself, adding them fails and
//! wry's own dialogs are used. The class is taken from an existing webview's delegate, not
//! looked up by name: objc2 registers it under a mangled name that changes with wry's version.
//!
//! `WebKit` records which methods the delegate implements when the delegate is set, so
//! [`install`] must run before the first service webview is created; the webview it is given
//! (the sidebar) keeps its defaults.
//!
//! What a dialog shows is decided by the [`DialogHandler`] in safe code (text, suppression, rate
//! limits); this module only converts between `WebKit` and `NSAlert`, shown as a sheet on the
//! webview's window so the main thread never blocks. `WebKit`'s completion handler is called
//! exactly once: with the default answer when the dialog is suppressed or its sheet never
//! completes.
//!
//! It also builds the `WebKit` configuration of service webviews (`configuration`), which
//! needs the same Objective-C calls, reports display wakes (`wake`) and reloads webviews from
//! their origin (`reload`).
//!
//! Every `unsafe` block says why it is sound.

mod configuration;
mod reload;
mod wake;

use std::cell::RefCell;
use std::ffi::CStr;
use std::fmt;
use std::ptr::{self, null_mut};
use std::sync::{Arc, OnceLock};

use block2::{DynBlock, RcBlock};
pub use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, NSObject, Sel};
use objc2::{ClassType, ffi, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSButton, NSControlStateValueOn,
    NSModalResponse, NSTextField,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{WKFrameInfo, WKWebView};
use tauri::webview::PlatformWebview;

use crate::{
    DialogAnswer, DialogHandler, DialogKind, DialogRequest, DialogText, PageOrigin, Presentation,
};

pub use configuration::{forbid_automatic_windows, service_configuration};
pub use reload::reload_from_origin;
pub use wake::observe_screens_wake;

#[derive(Debug)]
pub enum InstallError {
    AlreadyInstalled,
    /// The webview given to [`install`] has no UI delegate.
    NoDelegate,
    DonorClassExists,
    MissingDonorMethod(String),
    /// wry implements the method itself; its own handling is kept.
    AlreadyImplemented(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::AlreadyInstalled => write!(formatter, "dialogs are already installed"),
            InstallError::NoDelegate => write!(formatter, "the webview has no UI delegate"),
            InstallError::DonorClassExists => write!(
                formatter,
                "class {} exists already",
                DONOR_CLASS.to_string_lossy()
            ),
            InstallError::MissingDonorMethod(selector) => {
                write!(formatter, "donor class lacks {selector}")
            }
            InstallError::AlreadyImplemented(selector) => {
                write!(formatter, "wry implements {selector} itself")
            }
        }
    }
}

impl std::error::Error for InstallError {}

const DONOR_CLASS: &CStr = c"AtriumDialogMethods";
const SUPPRESSION_TITLE: &str = "Prevent this page from creating additional dialogs";
const PROMPT_FIELD_SIZE: (f64, f64) = (300.0, 24.0);

static HANDLER: OnceLock<Arc<dyn DialogHandler>> = OnceLock::new();

/// Adds the three panel methods to the class of `webview`'s UI delegate (wry's, shared by all
/// its webviews); see the crate documentation for when to call it. On an error nothing changes:
/// the pages keep getting the default answers.
pub fn install(
    webview: &PlatformWebview,
    handler: Arc<dyn DialogHandler>,
) -> Result<(), InstallError> {
    let target = delegate_class(webview)?;
    let donor = register_donor_class()?;
    HANDLER
        .set(handler)
        .map_err(|_| InstallError::AlreadyInstalled)?;
    for selector in panel_selectors() {
        copy_method(donor, target, selector)?;
    }
    Ok(())
}

fn delegate_class(webview: &PlatformWebview) -> Result<&'static AnyClass, InstallError> {
    let inner: *const WKWebView = webview.inner().cast();
    // SAFETY: `inner()` is the `WKWebView` Tauri built for this webview, alive while `webview`
    // is borrowed; `UIDelegate` is a plain property read on the main thread.
    let delegate = unsafe { (*inner).UIDelegate() }.ok_or(InstallError::NoDelegate)?;
    let object: *const AnyObject = Retained::as_ptr(&delegate).cast();
    // SAFETY: a `ProtocolObject` is an Objective-C object, retained by `delegate`.
    Ok(unsafe { (*object).class() })
}

fn panel_selectors() -> [Sel; 3] {
    [
        sel!(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:),
        sel!(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:),
        sel!(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:),
    ]
}

/// Registers the class holding the three implementations: objc2 derives their Objective-C type
/// encodings from the Rust signatures, which `copy_method` then reuses.
fn register_donor_class() -> Result<&'static AnyClass, InstallError> {
    let mut builder =
        ClassBuilder::new(DONOR_CLASS, NSObject::class()).ok_or(InstallError::DonorClassExists)?;
    let [alert, confirm, prompt] = panel_selectors();
    // SAFETY: each function has the parameter list WebKit uses for its selector (see the
    // `WKUIDelegate` declarations in objc2-web-kit): the web view, the message (plus the default
    // text for the prompt), the frame and the completion block. None of them touches the
    // receiver, so they work on wry's delegate as well.
    unsafe {
        builder.add_method(
            alert,
            run_alert_panel as extern "C-unwind" fn(_, _, _, _, _, _),
        );
        builder.add_method(
            confirm,
            run_confirm_panel as extern "C-unwind" fn(_, _, _, _, _, _),
        );
        builder.add_method(
            prompt,
            run_prompt_panel as extern "C-unwind" fn(_, _, _, _, _, _, _),
        );
    }
    Ok(builder.register())
}

/// Adds the method `selector` of `donor` to `target`, unless `target` defines it already.
fn copy_method(donor: &AnyClass, target: &AnyClass, selector: Sel) -> Result<(), InstallError> {
    let method = donor
        .instance_method(selector)
        .ok_or_else(|| InstallError::MissingDonorMethod(selector.to_string()))?;
    // SAFETY: `method` belongs to the registered donor class, so its type encoding is a C string
    // owned by the runtime for the life of the process; `target` is a registered class, and the
    // runtime refuses the addition (instead of replacing) when the class has the method.
    let added = unsafe {
        let types = ffi::method_getTypeEncoding(method);
        ffi::class_addMethod(
            ptr::from_ref(target).cast_mut(),
            selector,
            method.implementation(),
            types,
        )
    };
    if added.as_bool() {
        Ok(())
    } else {
        Err(InstallError::AlreadyImplemented(selector.to_string()))
    }
}

/// Key of the tag association; only its address matters.
static TAG_KEY: u8 = 0;

/// Attaches `tag` to the webview; the [`DialogRequest`]s of its pages carry it. The tag lives
/// as long as the webview.
pub fn tag_webview(webview: &PlatformWebview, tag: &str) {
    let object: *mut AnyObject = webview.inner().cast();
    let value = NSString::from_str(tag);
    // SAFETY: `inner()` is the `WKWebView` Tauri built for this webview, alive while `webview`
    // is borrowed; the association retains `value`, so the string lives as long as the web view,
    // and the static key is unique to this crate.
    unsafe {
        ffi::objc_setAssociatedObject(
            object,
            ptr::from_ref(&TAG_KEY).cast(),
            Retained::as_ptr(&value).cast_mut().cast(),
            ffi::OBJC_ASSOCIATION_RETAIN,
        );
    }
}

fn tag_of(webview: &WKWebView) -> Option<String> {
    // SAFETY: looks up the association `tag_webview` made on this live web view; the runtime
    // returns null or the `NSString` stored there, which the web view keeps alive.
    let value = unsafe {
        ffi::objc_getAssociatedObject(
            ptr::from_ref(webview).cast(),
            ptr::from_ref(&TAG_KEY).cast(),
        )
    };
    // SAFETY: non-null values are the `NSString` stored by `tag_webview` (see above).
    unsafe { value.cast::<NSString>().as_ref() }.map(ToString::to_string)
}

extern "C-unwind" fn run_alert_panel(
    _this: &AnyObject,
    _cmd: Sel,
    webview: &WKWebView,
    message: &NSString,
    frame: &WKFrameInfo,
    completion: &DynBlock<dyn Fn()>,
) {
    let completion = completion.copy();
    run_panel(webview, frame, DialogKind::Alert, message, move |_| {
        completion.call(());
    });
}

extern "C-unwind" fn run_confirm_panel(
    _this: &AnyObject,
    _cmd: Sel,
    webview: &WKWebView,
    message: &NSString,
    frame: &WKFrameInfo,
    completion: &DynBlock<dyn Fn(Bool)>,
) {
    let completion = completion.copy();
    run_panel(
        webview,
        frame,
        DialogKind::Confirm,
        message,
        move |answer| {
            completion.call((Bool::new(answer == DialogAnswer::Confirmed),));
        },
    );
}

extern "C-unwind" fn run_prompt_panel(
    _this: &AnyObject,
    _cmd: Sel,
    webview: &WKWebView,
    prompt: &NSString,
    default_text: Option<&NSString>,
    frame: &WKFrameInfo,
    completion: &DynBlock<dyn Fn(*mut NSString)>,
) {
    let completion = completion.copy();
    let kind = DialogKind::Prompt {
        default_text: default_text.map_or_default(ToString::to_string),
    };
    run_panel(webview, frame, kind, prompt, move |answer| match answer {
        DialogAnswer::Text(text) => {
            // WebKit copies the string during the call.
            let text = NSString::from_str(&text);
            completion.call((Retained::as_ptr(&text).cast_mut(),));
        }
        DialogAnswer::Cancelled | DialogAnswer::Confirmed => completion.call((null_mut(),)),
    });
}

/// Asks the handler, shows the panel as a sheet on the webview's window (app-modal without one)
/// and calls `finish` exactly once with the answer.
fn run_panel(
    webview: &WKWebView,
    frame: &WKFrameInfo,
    kind: DialogKind,
    message: &NSString,
    finish: impl FnOnce(DialogAnswer) + 'static,
) {
    let completion = Completion::new(finish);
    // Dropping `completion` unanswered gives WebKit the default answer.
    let (Some(mtm), Some(handler)) = (MainThreadMarker::new(), HANDLER.get()) else {
        return;
    };
    let request = DialogRequest {
        kind,
        message: message.to_string(),
        origin: page_origin(frame),
        webview_tag: tag_of(webview),
    };
    let Presentation::Show(text) = handler.present(&request) else {
        return;
    };
    let (alert, controls) = build_alert(mtm, &request.kind, &text);
    let handler = handler.clone();
    let answered = move |response: NSModalResponse| {
        let (answer, suppress_further) = controls.answer(response);
        handler.answered(&request, &answer, suppress_further);
        completion.answer(answer);
    };
    match webview.window() {
        Some(window) => {
            let block = RcBlock::new(answered);
            alert.beginSheetModalForWindow_completionHandler(&window, Some(&block));
        }
        None => answered(alert.runModal()),
    }
}

fn page_origin(frame: &WKFrameInfo) -> PageOrigin {
    // SAFETY: WebKit hands over a live frame on the main thread; the accessors are plain
    // property reads that objc2 only marks unsafe because it cannot check their nullability.
    let (host, port) = unsafe {
        let origin = frame.securityOrigin();
        (origin.host().to_string(), origin.port())
    };
    PageOrigin {
        host,
        port: u16::try_from(port).unwrap_or(0),
    }
}

/// The alert's controls whose state makes up the answer. Kept apart from the alert itself: the
/// sheet's completion block must not retain the alert, which retains the block.
struct PanelControls {
    text_field: Option<Retained<NSTextField>>,
    suppression_button: Option<Retained<NSButton>>,
}

impl PanelControls {
    fn answer(&self, response: NSModalResponse) -> (DialogAnswer, bool) {
        let suppress_further = self
            .suppression_button
            .as_ref()
            .is_some_and(|button| button.state() == NSControlStateValueOn);
        let answer = if response == NSAlertFirstButtonReturn {
            match &self.text_field {
                Some(field) => DialogAnswer::Text(field.stringValue().to_string()),
                None => DialogAnswer::Confirmed,
            }
        } else {
            DialogAnswer::Cancelled
        };
        (answer, suppress_further)
    }
}

/// Shows an app-modal warning with one OK button and returns once it is dismissed. Needs the
/// main thread (the marker proves it); blocks that thread, so call it from the running event
/// loop, not from a state the app needs to continue.
pub fn show_warning(mtm: MainThreadMarker, title: &str, message: &str) {
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    let _ok_button = alert.addButtonWithTitle(&NSString::from_str("OK"));
    alert.runModal();
}

fn build_alert(
    mtm: MainThreadMarker,
    kind: &DialogKind,
    text: &DialogText,
) -> (Retained<NSAlert>, PanelControls) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&text.title));
    alert.setInformativeText(&NSString::from_str(&text.message));
    let _ok_button = alert.addButtonWithTitle(&NSString::from_str("OK"));
    if *kind != DialogKind::Alert {
        let _cancel_button = alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    }
    let text_field = match kind {
        DialogKind::Prompt { default_text } => Some(add_text_field(&alert, default_text, mtm)),
        DialogKind::Alert | DialogKind::Confirm => None,
    };
    let suppression_button = text.offer_suppression.then(|| {
        alert.setShowsSuppressionButton(true);
        alert.suppressionButton()
    });
    let suppression_button = suppression_button.flatten();
    if let Some(button) = &suppression_button {
        button.setTitle(&NSString::from_str(SUPPRESSION_TITLE));
    }
    (
        alert,
        PanelControls {
            text_field,
            suppression_button,
        },
    )
}

fn add_text_field(
    alert: &NSAlert,
    default_text: &str,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&NSString::from_str(default_text), mtm);
    field.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(PROMPT_FIELD_SIZE.0, PROMPT_FIELD_SIZE.1),
    ));
    alert.setAccessoryView(Some(&field));
    // Typing should start right away, as in a browser.
    alert.layout();
    alert.window().setInitialFirstResponder(Some(&field));
    field
}

/// Answers `WebKit` exactly once: with the dialog's answer, or with the default when dropped
/// unanswered (the sheet's window closed before the user answered).
struct Completion {
    finish: RefCell<Option<Finish>>,
}

type Finish = Box<dyn FnOnce(DialogAnswer)>;

impl Completion {
    fn new(finish: impl FnOnce(DialogAnswer) + 'static) -> Completion {
        Completion {
            finish: RefCell::new(Some(Box::new(finish))),
        }
    }

    fn answer(&self, answer: DialogAnswer) {
        let finish = self.finish.borrow_mut().take();
        if let Some(finish) = finish {
            finish(answer);
        }
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        if let Some(finish) = self.finish.get_mut().take() {
            finish(DialogAnswer::Cancelled);
        }
    }
}
