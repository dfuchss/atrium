//! JavaScript dialogs (`alert`, `confirm`, `prompt`) of the service views and the windows they
//! open: what the native panel says and when a page gets none. [`Dialogs`] hands the policy to
//! the native dialog hooks (`native::install_dialogs`, `native::attach_dialogs`), whose panels
//! call [`DialogHandler`] on the main thread; the decisions are the [`DialogPolicy`]'s
//! (`policy`).
//!
//! - The title names the account (its sidebar label) and the host of the asking frame, taken
//!   from `WebKit`'s security origin so a page cannot pose as another.
//! - From a page's second dialog on, the panel offers "Prevent this page from creating
//!   additional dialogs"; ticked, the account's further dialogs get the default answer until
//!   its view loads a page again (also by reload).
//! - At most [`MAX_DIALOGS_PER_MINUTE`](policy::MAX_DIALOGS_PER_MINUTE) per account are shown;
//!   beyond that the default answer is given and logged.
//! - Dialogs of hidden accounts are shown right away, named: a sheet on Atrium's own window
//!   never covers other apps, while queueing would freeze the page's JavaScript (its messages
//!   and notifications) until the account is opened, without the user knowing why.

mod policy;

use std::sync::{Arc, Mutex, PoisonError};

use platform_ffi::{DialogAnswer, DialogHandler, DialogKind, DialogRequest, Presentation};
use protocol::Snapshot;
use tauri::{AppHandle, Runtime};
use uuid::Uuid;

use crate::labels::account_of_label;
use crate::logger::{Logger, now_ms};
use crate::native;
use policy::{DialogPolicy, account_names, origin_label};

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
        let decision = self
            .policy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .present(account, request, now_ms());
        let described = TauriDialogHandler::describe(request);
        match decision {
            Ok(text) => {
                self.logger.log(&format!("{described} -> shown"));
                Presentation::Show(text)
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
        self.policy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .answered(account, suppress_further);
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
            policy: Arc::new(Mutex::new(DialogPolicy::new())),
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
        self.policy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .page_loaded(account);
    }

    /// Takes the account names for the dialog titles from `snapshot`.
    pub(crate) fn set_names(&self, snapshot: &Snapshot) {
        self.policy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .set_names(account_names(snapshot));
    }
}
