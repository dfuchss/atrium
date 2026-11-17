//! Linux (`WebKitGTK`): native `alert`, `confirm` and `prompt` for pages in service webviews
//! and the windows they open.
//!
//! [`attach`] connects `WebKitGTK`'s `script-dialog` signal of a webview: what a dialog shows is
//! decided by the [`DialogHandler`] (text, suppression, rate limits), this module only converts
//! between `WebKit` and a GTK message dialog. Handling the signal keeps `WebKitGTK`'s own dialog
//! away. Before-unload prompts are confirmed without a dialog, as `WebKit` does on macOS, so a
//! page can neither keep itself open nor block quitting.
//!
//! The dialog is modal to the webview's window but does not block: it is answered from its
//! `response` signal, holding a reference to `WebKit`'s dialog, which `Pending` closes exactly
//! once (the asynchronous mode `WebKitGTK` documents for `script-dialog`). A nested main loop
//! (`gtk::Dialog::run`) would instead run the whole app's event handling, Tauri's included,
//! re-entrantly from inside `WebKit`'s signal for as long as the dialog is open.
//!
//! `WebKitGTK` names no frame for the dialog, so its origin is the top page's, not that of an
//! asking frame: the page last committed in the view. The view's current URI is no source, as
//! it already names the target of a navigation that has only started, while the old page's
//! scripts still run (a page could set `location` to another site and ask at once).

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use gtk::prelude::*;
use webkit2gtk::{LoadEvent, ScriptDialog, ScriptDialogType, WebView, WebViewExt};

use crate::{
    DialogAnswer, DialogHandler, DialogKind, DialogRequest, DialogText, PageOrigin, Presentation,
    origin_of,
};

#[derive(Debug)]
pub enum InstallError {
    AlreadyInstalled,
    /// [`attach`] was called before [`install`].
    NotInstalled,
}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::AlreadyInstalled => write!(formatter, "dialogs are already installed"),
            InstallError::NotInstalled => write!(formatter, "dialogs are not installed"),
        }
    }
}

impl std::error::Error for InstallError {}

const SUPPRESSION_TITLE: &str = "Prevent this page from creating additional dialogs";

static HANDLER: OnceLock<Arc<dyn DialogHandler>> = OnceLock::new();

/// Sets the handler deciding the dialogs of every webview given to [`attach`] later.
pub fn install(handler: Arc<dyn DialogHandler>) -> Result<(), InstallError> {
    HANDLER
        .set(handler)
        .map_err(|_| InstallError::AlreadyInstalled)
}

/// Shows the dialogs of `view`'s pages as decided by the installed handler; their
/// [`DialogRequest`]s carry `tag`. Call it on the main thread.
pub fn attach(view: &WebView, tag: &str) -> Result<(), InstallError> {
    let handler = HANDLER.get().ok_or(InstallError::NotInstalled)?.clone();
    let tag = tag.to_string();
    let page = CommittedPage::default();
    // Attached to an idle view, its URI is that of the committed page.
    if !view.is_loading() {
        page.observe(LoadEvent::Committed, view.uri().as_deref());
    }
    let observed = page.clone();
    view.connect_load_changed(move |view, event| observed.observe(event, view.uri().as_deref()));
    view.connect_script_dialog(move |view, dialog| {
        on_script_dialog(view, dialog, &page.origin(), &handler, &tag)
    });
    Ok(())
}

/// The URI of the page last committed in a view, shared by its signal handlers (main thread).
#[derive(Clone, Default)]
struct CommittedPage {
    uri: Rc<RefCell<Option<String>>>,
}

impl CommittedPage {
    /// Takes `uri`, the view's URI at `event`, if the event commits a page.
    fn observe(&self, event: LoadEvent, uri: Option<&str>) {
        if event == LoadEvent::Committed {
            *self.uri.borrow_mut() = uri.map(String::from);
        }
    }

    /// Origin of the committed page; none (no host, "a page") before the first commit.
    fn origin(&self) -> PageOrigin {
        origin_of(self.uri.borrow().as_deref().unwrap_or_default())
    }
}

/// Handles one `script-dialog` emission; `true` keeps `WebKitGTK`'s own dialog away.
fn on_script_dialog(
    view: &WebView,
    dialog: &ScriptDialog,
    origin: &PageOrigin,
    handler: &Arc<dyn DialogHandler>,
    tag: &str,
) -> bool {
    let dialog_type = dialog.dialog_type();
    if dialog_type == ScriptDialogType::BeforeUnloadConfirm {
        dialog.confirm_set_confirmed(true);
        return true;
    }
    let Some(kind) = kind_of(dialog_type, || {
        dialog
            .prompt_get_default_text()
            .map_or_default(String::from)
    }) else {
        return false;
    };
    let request = DialogRequest {
        kind,
        message: dialog.message().map_or_default(String::from),
        origin: origin.clone(),
        webview_tag: Some(tag.to_string()),
    };
    match handler.present(&request) {
        // Answered with the default once the signal returns, as nothing holds the dialog.
        Presentation::Suppress => set_answer(dialog, &request.kind, &DialogAnswer::Cancelled),
        Presentation::Show(text) => show(view, Pending::new(dialog, request), &text, handler),
    }
    true
}

/// The dialog kind of a `WebKit` dialog type; None for types this module leaves to `WebKit`.
/// `default_text` is only asked for prompts.
fn kind_of(
    dialog_type: ScriptDialogType,
    default_text: impl FnOnce() -> String,
) -> Option<DialogKind> {
    match dialog_type {
        ScriptDialogType::Alert => Some(DialogKind::Alert),
        ScriptDialogType::Confirm => Some(DialogKind::Confirm),
        ScriptDialogType::Prompt => Some(DialogKind::Prompt {
            default_text: default_text(),
        }),
        _ => None,
    }
}

/// Sets `answer` on `WebKit`'s dialog; what is not set keeps `WebKit`'s default (`confirm` ->
/// false, `prompt` -> null).
fn set_answer(dialog: &ScriptDialog, kind: &DialogKind, answer: &DialogAnswer) {
    match (kind, answer) {
        (DialogKind::Confirm, answer) => {
            dialog.confirm_set_confirmed(*answer == DialogAnswer::Confirmed);
        }
        (DialogKind::Prompt { .. }, DialogAnswer::Text(text)) => dialog.prompt_set_text(text),
        _ => {}
    }
}

/// Shows the message dialog for `pending` and answers it from the dialog's response.
///
/// The dialog of a hidden view (an account not on screen) is shown at once too, but takes no
/// keys meant for the visible account: nothing in it has the focus and Enter does not confirm,
/// so typing that goes on cannot fill its prompt or answer it.
fn show(view: &WebView, pending: Pending, text: &DialogText, handler: &Arc<dyn DialogHandler>) {
    let parent = view
        .toplevel()
        .and_then(|widget| widget.downcast::<gtk::Window>().ok());
    let takes_keys = view.is_visible();
    let (panel, controls) = build_panel(parent.as_ref(), &pending.request.kind, text, takes_keys);
    let entry = controls.entry.clone();
    let handler = handler.clone();
    panel.connect_response(move |panel, response| {
        // Closing the panel emits a second response, which finds nothing left to answer.
        let Some(script_dialog) = pending.take() else {
            return;
        };
        let (answer, suppress_further) = controls.answer(response);
        handler.answered(&pending.request, &answer, suppress_further);
        finish(&script_dialog, &pending.request.kind, &answer);
        panel.close();
    });
    panel.show_all();
    if !takes_keys {
        // Showing it focused its first control, the prompt's entry.
        panel.set_focus(None::<&gtk::Widget>);
    } else if let Some(entry) = entry {
        // Typing should start right away, as in a browser.
        entry.grab_focus();
    }
}

/// The panel for a dialog of `kind`; OK is its default response only if it `takes_keys`.
fn build_panel(
    parent: Option<&gtk::Window>,
    kind: &DialogKind,
    text: &DialogText,
    takes_keys: bool,
) -> (gtk::MessageDialog, PanelControls) {
    let panel = gtk::MessageDialog::new(
        parent,
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        message_type(kind),
        gtk::ButtonsType::None,
        &text.title,
    );
    panel.set_title("Atrium");
    panel.set_secondary_text(Some(&text.message));
    if *kind != DialogKind::Alert {
        panel.add_button("_Cancel", gtk::ResponseType::Cancel);
    }
    panel.add_button("_OK", gtk::ResponseType::Ok);
    if takes_keys {
        panel.set_default_response(gtk::ResponseType::Ok);
    }
    let area = panel.content_area();
    let entry = match kind {
        DialogKind::Prompt { default_text } => Some(add_entry(&area, default_text)),
        DialogKind::Alert | DialogKind::Confirm => None,
    };
    let suppression_button = text.offer_suppression.then(|| {
        let button = gtk::CheckButton::with_label(SUPPRESSION_TITLE);
        area.add(&button);
        button
    });
    (
        panel,
        PanelControls {
            entry,
            suppression_button,
        },
    )
}

fn message_type(kind: &DialogKind) -> gtk::MessageType {
    match kind {
        DialogKind::Alert => gtk::MessageType::Info,
        DialogKind::Confirm | DialogKind::Prompt { .. } => gtk::MessageType::Question,
    }
}

fn add_entry(area: &gtk::Box, default_text: &str) -> gtk::Entry {
    let entry = gtk::Entry::new();
    entry.set_text(default_text);
    // Enter confirms, as in a browser.
    entry.set_activates_default(true);
    area.add(&entry);
    entry
}

/// The panel's controls whose state makes up the answer.
struct PanelControls {
    entry: Option<gtk::Entry>,
    suppression_button: Option<gtk::CheckButton>,
}

impl PanelControls {
    fn answer(&self, response: gtk::ResponseType) -> (DialogAnswer, bool) {
        let suppress_further = self
            .suppression_button
            .as_ref()
            .is_some_and(ToggleButtonExt::is_active);
        let entered = self.entry.as_ref().map(|entry| entry.text().to_string());
        (answer_for(response, entered), suppress_further)
    }
}

/// OK answers with the entered text (prompts) or as confirmed; anything else (Cancel, Escape,
/// closing the panel) cancels.
fn answer_for(response: gtk::ResponseType, entered: Option<String>) -> DialogAnswer {
    match (response, entered) {
        (gtk::ResponseType::Ok, Some(text)) => DialogAnswer::Text(text),
        (gtk::ResponseType::Ok, None) => DialogAnswer::Confirmed,
        _ => DialogAnswer::Cancelled,
    }
}

/// Sets `answer` and hands `WebKit`'s dialog back, which resumes the page.
fn finish(script_dialog: &ScriptDialog, kind: &DialogKind, answer: &DialogAnswer) {
    set_answer(script_dialog, kind, answer);
    script_dialog.close();
}

/// A shown dialog's request with the reference to `WebKit`'s dialog that keeps it open. Taken
/// at most once; dropped untaken (the panel destroyed with its window), it gives `WebKit` the
/// default answer.
struct Pending {
    request: DialogRequest,
    script_dialog: RefCell<Option<ScriptDialog>>,
}

impl Pending {
    fn new(script_dialog: &ScriptDialog, request: DialogRequest) -> Pending {
        Pending {
            request,
            script_dialog: RefCell::new(Some(script_dialog.clone())),
        }
    }

    fn take(&self) -> Option<ScriptDialog> {
        self.script_dialog.borrow_mut().take()
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(script_dialog) = self.script_dialog.get_mut().take() {
            finish(&script_dialog, &self.request.kind, &DialogAnswer::Cancelled);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_origin_is_the_committed_page() {
        let page = CommittedPage::default();
        assert_eq!(page.origin().host, "");
        page.observe(LoadEvent::Committed, Some("https://chat.example.org/"));
        page.observe(LoadEvent::Started, Some("https://accounts.example.com/"));
        page.observe(LoadEvent::Redirected, Some("https://login.example.com/"));
        assert_eq!(page.origin().host, "chat.example.org");
        page.observe(LoadEvent::Committed, Some("https://login.example.com/"));
        assert_eq!(page.origin().host, "login.example.com");
        page.observe(LoadEvent::Finished, Some("https://other.example.com/"));
        assert_eq!(page.origin().host, "login.example.com");
    }

    #[test]
    fn ok_answers_with_the_text_or_confirms() {
        assert_eq!(
            answer_for(gtk::ResponseType::Ok, Some("typed".into())),
            DialogAnswer::Text("typed".into())
        );
        assert_eq!(
            answer_for(gtk::ResponseType::Ok, None),
            DialogAnswer::Confirmed
        );
    }

    #[test]
    fn everything_but_ok_cancels() {
        for response in [gtk::ResponseType::Cancel, gtk::ResponseType::DeleteEvent] {
            assert_eq!(
                answer_for(response, Some("typed".into())),
                DialogAnswer::Cancelled
            );
            assert_eq!(answer_for(response, None), DialogAnswer::Cancelled);
        }
    }

    #[test]
    fn kinds_follow_the_webkit_types() {
        assert_eq!(
            kind_of(ScriptDialogType::Alert, || unreachable!()),
            Some(DialogKind::Alert)
        );
        assert_eq!(
            kind_of(ScriptDialogType::Confirm, || unreachable!()),
            Some(DialogKind::Confirm)
        );
        assert_eq!(
            kind_of(ScriptDialogType::Prompt, || "default".into()),
            Some(DialogKind::Prompt {
                default_text: "default".into()
            })
        );
        assert_eq!(
            kind_of(ScriptDialogType::BeforeUnloadConfirm, || unreachable!()),
            None
        );
    }
}
