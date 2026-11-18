//! JavaScript dialogs (`alert`, `confirm`, `prompt`) of the service views and the windows they
//! open: what the native panel says and when a page gets none. [`Dialogs`] hands the domain's
//! [`DialogPolicy`] to the native dialog hooks (`native::install_dialogs`,
//! `native::attach_dialogs`), whose panels call [`DialogHandler`] on the main thread.
//!
//! - The host in the title is taken from `WebKit`'s security origin, so a page cannot pose as
//!   another.
//! - The panel offers the suppression as "Prevent this page from creating additional dialogs";
//!   a reload also counts as a page load that ends it.
//! - Withheld dialogs get the default answer and are logged.
//! - Dialogs of hidden accounts are shown right away, named: a sheet on Atrium's own window
//!   never covers other apps, while queueing would freeze the page's JavaScript (its messages
//!   and notifications) until the account is opened, without the user knowing why.

use std::sync::{Arc, Mutex};

use domain::{DialogPolicy, dialog_origin_label};
use fs_util::lock_ignoring_poison;
use platform_ffi::{
    DialogAnswer, DialogHandler, DialogKind, DialogRequest, DialogText, PageOrigin, Presentation,
};
use protocol::Snapshot;
use tauri::{AppHandle, Runtime};
use uuid::Uuid;

use crate::labels::account_of_label;
use crate::logger::{Logger, now_ms};
use crate::native;

/// Shared policy state: written by the actor (snapshots, page loads) and the main thread
/// (dialogs), held only briefly.
type SharedDialogPolicy = Arc<Mutex<DialogPolicy>>;

fn kind_name(kind: &DialogKind) -> &'static str {
    match kind {
        DialogKind::Alert => "alert",
        DialogKind::Confirm => "confirm",
        DialogKind::Prompt { .. } => "prompt",
    }
}

fn origin_label(origin: &PageOrigin) -> Option<String> {
    dialog_origin_label(&origin.host, origin.port)
}

/// Answer for the log: never the entered text itself.
fn answer_name(answer: &DialogAnswer) -> String {
    match answer {
        DialogAnswer::Cancelled => "cancelled".into(),
        DialogAnswer::Confirmed => "OK".into(),
        DialogAnswer::Text(text) => format!("text ({} chars)", text.chars().count()),
    }
}

/// The [`DialogHandler`] of the native hooks: applies the [`DialogPolicy`] and logs each dialog,
/// never its message.
struct TauriDialogHandler {
    policy: SharedDialogPolicy,
    logger: Logger,
}

impl TauriDialogHandler {
    fn describe(request: &DialogRequest) -> String {
        let account = request
            .webview_tag
            .as_deref()
            .and_then(account_of_label)
            .map_or_else(|| "-".to_string(), |id| id.to_string());
        let origin = origin_label(&request.origin).unwrap_or_else(|| "-".into());
        format!("[{account}] {} from {origin}", kind_name(&request.kind))
    }
}

impl DialogHandler for TauriDialogHandler {
    fn present(&self, request: &DialogRequest) -> Presentation {
        let account = request.webview_tag.as_deref().and_then(account_of_label);
        let decision = lock_ignoring_poison(&self.policy).present(
            account,
            origin_label(&request.origin).as_deref(),
            &request.message,
            now_ms(),
        );
        let described = TauriDialogHandler::describe(request);
        match decision {
            Ok(text) => {
                self.logger.log(&format!("{described} -> shown"));
                Presentation::Show(DialogText {
                    title: text.title,
                    message: text.message,
                    offer_suppression: text.offer_suppression,
                })
            }
            Err(reason) => {
                self.logger
                    .log(&format!("{described} -> default answer ({reason:?})"));
                Presentation::Suppress
            }
        }
    }

    fn answered(&self, request: &DialogRequest, answer: &DialogAnswer, suppress_further: bool) {
        let account = request.webview_tag.as_deref().and_then(account_of_label);
        lock_ignoring_poison(&self.policy).answered(account, suppress_further);
        let further = if suppress_further {
            ", no further dialogs until the next page load"
        } else {
            ""
        };
        self.logger.log(&format!(
            "{} -> {}{further}",
            TauriDialogHandler::describe(request),
            answer_name(answer)
        ));
    }
}

/// JavaScript dialogs of the service views and their popups: native panels that follow the
/// [`DialogPolicy`], shown by the native dialog hooks.
#[derive(Clone)]
pub(crate) struct Dialogs {
    policy: SharedDialogPolicy,
}

impl Dialogs {
    pub(crate) fn new() -> Dialogs {
        Dialogs {
            policy: Arc::default(),
        }
    }

    /// Makes `alert`, `confirm` and `prompt` of the service views follow the policy (see
    /// `native::install_dialogs` for when to call it).
    pub(crate) fn install<R: Runtime>(&self, app: &AppHandle<R>, logger: &Logger) {
        let handler = Arc::new(TauriDialogHandler {
            policy: self.policy.clone(),
            logger: logger.clone(),
        });
        native::install_dialogs(app, handler, logger);
    }

    /// The view of `account` loaded a page: earlier dialogs and a suppression no longer count.
    pub(crate) fn page_loaded(&self, account: Uuid) {
        lock_ignoring_poison(&self.policy).page_loaded(account);
    }

    /// Takes the account names for the dialog titles from `snapshot`.
    pub(crate) fn set_names(&self, snapshot: &Snapshot) {
        lock_ignoring_poison(&self.policy).set_names(snapshot);
    }
}
