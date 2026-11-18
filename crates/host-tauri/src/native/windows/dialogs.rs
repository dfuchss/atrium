//! JavaScript dialogs: Atrium's own, decided by the shared policy of `dialogs` (see
//! `platform_ffi::windows`), for the service views and the windows they open.
//!
//! Each shown dialog gets a small window of Atrium's UI (`script-dialog-<id>`, see
//! `protocol::script_dialog`): owned by the asking webview's window, so it stays above it,
//! without a taskbar button, and centred over its owner once the UI has laid it out. The window
//! talks to the `script-dialog` plugin, which the capability of the same name grants to these
//! windows only; each window reaches only its own dialog, named by its label, and only while
//! that dialog waits. Closing the window cancels the dialog.
//!
//! A dialog of a hidden account is shown at once too (see `dialogs`), but takes no keys meant
//! for the visible account (see [`ScriptDialog::takes_keys`]). The window is not modal: the
//! page that asked waits for the answer anyway.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use fs_util::lock_ignoring_poison;
use platform_ffi::{DialogAnswer, DialogHandler, DialogKind, ShownDialog};
use protocol::{SCRIPT_DIALOG_QUERY, ScriptDialog, ScriptDialogAnswer, ScriptDialogKind};
use tauri::plugin::TauriPlugin;
use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, Runtime, State, Webview, WebviewUrl,
    WebviewWindowBuilder, Window, WindowEvent,
};

use crate::Logger;
use crate::logger::spawn_logged;
use crate::native::with_platform_webview;

/// Name of the plugin the dialog windows answer through (see `protocol::script_dialog`).
const PLUGIN: &str = "script-dialog";
const LABEL_PREFIX: &str = "script-dialog-";
/// Width of a dialog window; its height follows the content.
const WIDTH: f64 = 440.0;
/// Height of a dialog window while the UI lays it out, hidden.
const INITIAL_HEIGHT: f64 = 160.0;
const MAX_HEIGHT: f64 = 720.0;

struct OpenDialog {
    dialog: ScriptDialog,
    /// Label of the window the dialog is centred over.
    owner: Option<String>,
}

/// The dialogs whose windows are open, by id; shared by the windows' events and the plugin.
#[derive(Default)]
struct OpenDialogs(Mutex<HashMap<u64, OpenDialog>>);

impl OpenDialogs {
    fn with<T>(&self, access: impl FnOnce(&mut HashMap<u64, OpenDialog>) -> T) -> T {
        access(&mut lock_ignoring_poison(&self.0))
    }
}

/// Makes `alert`, `confirm` and `prompt` of the webviews attached from now on show Atrium's
/// dialog windows, decided by `handler`.
pub(crate) fn install_dialogs<R: Runtime>(
    app: &AppHandle<R>,
    handler: Arc<dyn DialogHandler>,
    logger: &Logger,
) {
    app.manage(OpenDialogs::default());
    if let Err(error) = app.plugin(plugin()) {
        logger.log(&format!("JavaScript dialogs unavailable: {error}"));
        return;
    }
    let (show_app, show_logger) = (app.clone(), logger.clone());
    let show = Box::new(move |shown: ShownDialog| open_later(&show_app, shown, &show_logger));
    match platform_ffi::install(handler, show) {
        Ok(()) => logger.log("JavaScript dialogs: Atrium's own"),
        Err(error) => logger.log(&format!("JavaScript dialogs unavailable: {error}")),
    }
}

/// Shows the dialogs of `webview` through the installed handler; they carry its label (see
/// `labels::account_of_label`).
pub(crate) fn attach_dialogs<R: Runtime>(webview: &Webview<R>, logger: &Logger) {
    let label = webview.label().to_string();
    with_platform_webview(webview, logger, "cannot attach dialogs", move |platform| {
        platform_ffi::attach(&platform.controller(), &label)
    });
}

/// The commands of the dialog windows. They run on Tauri's thread pool (`async`): run on the
/// main thread, inside `WebView2`'s message event, showing their window never returned.
fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new(PLUGIN)
        .invoke_handler(tauri::generate_handler![get, show, answer])
        .build()
}

/// Opens the window of `shown` from a thread of its own: `WebView2` must not create a webview
/// inside its event, which waits for this call.
fn open_later<R: Runtime>(app: &AppHandle<R>, shown: ShownDialog, logger: &Logger) {
    let (app, window_logger) = (app.clone(), logger.clone());
    spawn_logged("script-dialog", logger, move || {
        if let Err(error) = open_window(&app, &shown) {
            window_logger.log(&format!("cannot show a dialog window: {error}"));
            finish(&app, shown.id, DialogAnswer::Cancelled, false);
        }
    });
}

/// Opens the hidden window of `shown`; the UI shows it once laid out (see [`show`]).
fn open_window<R: Runtime>(app: &AppHandle<R>, shown: &ShownDialog) -> tauri::Result<()> {
    let owner = shown
        .request
        .webview_tag
        .as_deref()
        .and_then(|label| app.get_webview(label))
        .map(|webview| webview.window());
    app.state::<OpenDialogs>().with(|dialogs| {
        dialogs.insert(
            shown.id,
            OpenDialog {
                dialog: script_dialog(shown),
                owner: owner.as_ref().map(|window| window.label().to_string()),
            },
        )
    });
    let label = format!("{LABEL_PREFIX}{}", shown.id);
    let url = WebviewUrl::App(format!("index.html?{SCRIPT_DIALOG_QUERY}").into());
    let mut builder = WebviewWindowBuilder::new(app, &label, url)
        .title("Atrium")
        .inner_size(WIDTH, INITIAL_HEIGHT)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .skip_taskbar(true)
        // A focused webview takes the focus when created, even in the hidden window.
        .focused(shown.takes_keys)
        .visible(false);
    if let Some(owner) = &owner {
        builder = builder.owner_raw(owner.hwnd()?);
    }
    let window = builder.build()?;
    let (closing_app, id) = (app.clone(), shown.id);
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            finish(&closing_app, id, DialogAnswer::Cancelled, false);
        }
    });
    Ok(())
}

/// What the window of `shown` shows.
fn script_dialog(shown: &ShownDialog) -> ScriptDialog {
    ScriptDialog {
        title: shown.text.title.clone(),
        message: shown.text.message.clone(),
        kind: match &shown.request.kind {
            DialogKind::Alert => ScriptDialogKind::Alert,
            DialogKind::Confirm => ScriptDialogKind::Confirm,
            DialogKind::Prompt { default_text } => ScriptDialogKind::Prompt {
                default_text: default_text.clone(),
            },
        },
        offer_suppression: shown.text.offer_suppression,
        takes_keys: shown.takes_keys,
    }
}

/// The dialog answer for the user's `answer` to a dialog of `kind`.
fn dialog_answer(kind: &ScriptDialogKind, answer: ScriptDialogAnswer) -> DialogAnswer {
    let entered = matches!(kind, ScriptDialogKind::Prompt { .. }).then_some(answer.text);
    DialogAnswer::from_ok(answer.confirmed, entered)
}

/// Ends dialog `id` with `answer` unless it has ended already: forgets its window and resumes
/// its page on the main thread, where `WebView2` waits.
fn finish<R: Runtime>(app: &AppHandle<R>, id: u64, answer: DialogAnswer, suppress_further: bool) {
    let open = app
        .state::<OpenDialogs>()
        .with(|dialogs| dialogs.remove(&id));
    if open.is_some() {
        // Fails only once the event loop has ended, and the page with it.
        let _ = app.run_on_main_thread(move || {
            platform_ffi::answer(id, &answer, suppress_further);
        });
    }
}

/// Id of the dialog of the window `webview` belongs to; the calling window names it, the page
/// cannot pick another.
fn own_dialog<R: Runtime>(webview: &Webview<R>) -> Result<u64, String> {
    webview
        .label()
        .strip_prefix(LABEL_PREFIX)
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| "not a dialog window".to_string())
}

const ENDED: &str = "the dialog has ended";

/// The dialog of the calling window.
#[tauri::command(async)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]
fn get<R: Runtime>(
    webview: Webview<R>,
    dialogs: State<'_, OpenDialogs>,
) -> Result<ScriptDialog, String> {
    let id = own_dialog(&webview)?;
    dialogs
        .with(|dialogs| dialogs.get(&id).map(|open| open.dialog.clone()))
        .ok_or_else(|| ENDED.to_string())
}

/// Sizes the calling window to `height` (its laid-out content), centres it over its owner and
/// shows it.
#[tauri::command(async)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]
fn show<R: Runtime>(
    webview: Webview<R>,
    dialogs: State<'_, OpenDialogs>,
    height: f64,
) -> Result<(), String> {
    let id = own_dialog(&webview)?;
    let (owner, takes_keys) = dialogs
        .with(|dialogs| {
            let open = dialogs.get(&id)?;
            Some((open.owner.clone(), open.dialog.takes_keys))
        })
        .ok_or_else(|| ENDED.to_string())?;
    let window = webview.window();
    let owner = owner.and_then(|label| webview.app_handle().get_window(&label));
    present(&window, owner.as_ref(), height, takes_keys).map_err(|error| error.to_string())
}

/// Answers the dialog of the calling window and closes it.
#[tauri::command(async)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]
fn answer<R: Runtime>(
    webview: Webview<R>,
    dialogs: State<'_, OpenDialogs>,
    answer: ScriptDialogAnswer,
) -> Result<(), String> {
    let id = own_dialog(&webview)?;
    let suppress_further = answer.suppress_further;
    let answer = dialogs
        .with(|dialogs| {
            let open = dialogs.get(&id)?;
            Some(dialog_answer(&open.dialog.kind, answer))
        })
        .ok_or_else(|| ENDED.to_string())?;
    finish(webview.app_handle(), id, answer, suppress_further);
    webview.window().close().map_err(|error| error.to_string())
}

/// Gives `window` the content `height`, centres it over `owner` (else the screen) and shows
/// it: with the focus if it `takes_keys`, else without taking it from the visible account.
fn present<R: Runtime>(
    window: &Window<R>,
    owner: Option<&Window<R>>,
    height: f64,
    takes_keys: bool,
) -> tauri::Result<()> {
    window.set_size(LogicalSize::new(WIDTH, height.clamp(1.0, MAX_HEIGHT)))?;
    match owner {
        Some(owner) => window.set_position(centred_over(window, owner)?)?,
        None => window.center()?,
    }
    if takes_keys {
        window.show()?;
        window.set_focus()?;
    } else {
        platform_ffi::show_without_activating(window.hwnd()?);
    }
    Ok(())
}

/// Position of `window` centred over `owner`.
fn centred_over<R: Runtime>(
    window: &Window<R>,
    owner: &Window<R>,
) -> tauri::Result<PhysicalPosition<i32>> {
    let (origin, size, own) = (
        owner.outer_position()?,
        owner.outer_size()?,
        window.outer_size()?,
    );
    let offset = |outer: u32, inner: u32| {
        i32::try_from((i64::from(outer) - i64::from(inner)) / 2).unwrap_or_default()
    };
    Ok(PhysicalPosition::new(
        origin.x + offset(size.width, own.width),
        origin.y + offset(size.height, own.height),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(confirmed: bool) -> ScriptDialogAnswer {
        ScriptDialogAnswer {
            confirmed,
            text: "typed".into(),
            suppress_further: false,
        }
    }

    #[test]
    fn ok_answers_a_prompt_with_the_text_and_confirms_the_others() {
        let prompt = ScriptDialogKind::Prompt {
            default_text: String::new(),
        };
        assert_eq!(
            dialog_answer(&prompt, answer(true)),
            DialogAnswer::Text("typed".into())
        );
        for kind in [ScriptDialogKind::Alert, ScriptDialogKind::Confirm] {
            assert_eq!(dialog_answer(&kind, answer(true)), DialogAnswer::Confirmed);
        }
    }

    #[test]
    fn cancel_cancels_every_kind() {
        let prompt = ScriptDialogKind::Prompt {
            default_text: String::new(),
        };
        for kind in [ScriptDialogKind::Alert, ScriptDialogKind::Confirm, prompt] {
            assert_eq!(dialog_answer(&kind, answer(false)), DialogAnswer::Cancelled);
        }
    }
}
