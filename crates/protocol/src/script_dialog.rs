//! The window of a JavaScript dialog of a service page (Windows): the host opens the UI with
//! [`SCRIPT_DIALOG_QUERY`] in a window of its own per shown dialog, which talks to the
//! `script-dialog` plugin only, and only about its own dialog.
//!
//! - UI initial load: [`CMD_SCRIPT_DIALOG_GET`] returns a `Result<ScriptDialog, String>`.
//! - UI rendered: [`CMD_SCRIPT_DIALOG_SHOW`] with `{ height }` (of the content, in CSS pixels)
//!   sizes the window to it and shows it.
//! - UI -> backend: [`CMD_SCRIPT_DIALOG_ANSWER`] with `{ answer: ScriptDialogAnswer }` answers
//!   the dialog and closes the window. Closing the window otherwise cancels the dialog.

use serde::{Deserialize, Serialize};

/// Query parameter that makes the UI show the dialog of its window instead of the sidebar.
pub const SCRIPT_DIALOG_QUERY: &str = "script-dialog";
pub const CMD_SCRIPT_DIALOG_GET: &str = "plugin:script-dialog|get";
pub const CMD_SCRIPT_DIALOG_SHOW: &str = "plugin:script-dialog|show";
pub const CMD_SCRIPT_DIALOG_ANSWER: &str = "plugin:script-dialog|answer";
/// Label of the checkbox a dialog offers to suppress the page's further dialogs, on every
/// platform.
pub const SUPPRESSION_TITLE: &str = "Prevent this page from creating additional dialogs";

/// What the dialog window shows. The texts come from the page: shown as text, never as HTML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptDialog {
    /// `"<account>: <host> says"`.
    pub title: String,
    pub message: String,
    pub kind: ScriptDialogKind,
    /// Offer "Prevent this page from creating additional dialogs".
    pub offer_suppression: bool,
    /// The asking page is on screen. A dialog of a hidden account takes no keys: nothing in
    /// it has the focus, and neither Enter nor Escape answers it.
    pub takes_keys: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScriptDialogKind {
    Alert,
    Confirm,
    Prompt { default_text: String },
}

/// The user's answer: OK (with the entered text of a prompt) or Cancel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptDialogAnswer {
    pub confirmed: bool,
    /// The entered text; only read for a confirmed prompt.
    pub text: String,
    pub suppress_further: bool,
}
