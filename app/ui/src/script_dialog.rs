//! The window of a JavaScript dialog of a service page (Windows, see
//! `protocol::script_dialog`): the title naming account and site, the page's message as plain
//! text, a text field for a prompt, the suppression checkbox from a page's second dialog on,
//! and OK / Cancel. Enter answers OK and Escape cancels, only in a dialog that takes keys.

use leptos::prelude::*;
use protocol::{
    CMD_SCRIPT_DIALOG_ANSWER, CMD_SCRIPT_DIALOG_GET, CMD_SCRIPT_DIALOG_SHOW, ScriptDialog,
    ScriptDialogAnswer, ScriptDialogKind,
};
use serde::Serialize;
use wasm_bindgen::{JsCast, JsValue};

use crate::tauri;

const SUPPRESSION_TITLE: &str = "Prevent this page from creating additional dialogs";
const PANEL_ID: &str = "script-dialog";
const TEXT_ID: &str = "script-dialog-text";
const OK_ID: &str = "script-dialog-ok";

fn log_error(message: &str) {
    web_sys::console::error_1(&JsValue::from_str(message));
}

/// The dialog of this window, once loaded from the host.
#[component]
pub(crate) fn ScriptDialogWindow() -> impl IntoView {
    let dialog = RwSignal::new(None::<ScriptDialog>);
    leptos::task::spawn_local(async move {
        match tauri::call::<ScriptDialog>(CMD_SCRIPT_DIALOG_GET, ()).await {
            Ok(loaded) => dialog.set(Some(loaded)),
            Err(error) => log_error(&error),
        }
    });
    move || {
        dialog
            .get()
            .map(|dialog| view! { <DialogPanel dialog=dialog /> })
    }
}

#[component]
fn DialogPanel(dialog: ScriptDialog) -> impl IntoView {
    let ScriptDialog {
        title,
        message,
        kind,
        offer_suppression,
        takes_keys,
    } = dialog;
    let default_text = match &kind {
        ScriptDialogKind::Prompt { default_text } => Some(default_text.clone()),
        ScriptDialogKind::Alert | ScriptDialogKind::Confirm => None,
    };
    let text = RwSignal::new(default_text.clone().unwrap_or_default());
    let suppress = RwSignal::new(false);
    let respond = move |confirmed: bool| {
        send_answer(ScriptDialogAnswer {
            confirmed,
            text: text.get_untracked(),
            suppress_further: suppress.get_untracked(),
        });
    };
    if takes_keys {
        let on_key = window_event_listener(leptos::ev::keydown, move |event| {
            if event.key() == "Escape" {
                respond(false);
            }
        });
        on_cleanup(move || on_key.remove());
    }
    request_animation_frame(move || present(takes_keys));
    view! {
        <form class="script-dialog" id=PANEL_ID on:submit=move |event| { event.prevent_default(); respond(true); }>
            <h1>{title}</h1>
            <p class="message">{message}</p>
            {default_text.map(|_| view! {
                <input type="text" id=TEXT_ID autocomplete="off" spellcheck="false"
                    prop:value=move || text.get() on:input=move |event| text.set(event_target_value(&event)) />
            })}
            {offer_suppression.then(|| view! {
                <label class="check">
                    <input type="checkbox" prop:checked=move || suppress.get() on:change=move |event| suppress.set(event_target_checked(&event)) />
                    {SUPPRESSION_TITLE}
                </label>
            })}
            <div class="actions">
                {(kind != ScriptDialogKind::Alert).then(|| view! {
                    <button class="btn" type="button" on:click=move |_| respond(false)>"Cancel"</button>
                })}
                <button class="btn primary" type="submit" id=OK_ID>"OK"</button>
            </div>
        </form>
    }
}

/// Focuses the text field (its text selected) or OK if the dialog takes keys, then has the
/// host fit the window to the laid-out panel.
fn present(takes_keys: bool) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    if takes_keys {
        focus_first(&document);
    }
    let height = document.get_element_by_id(PANEL_ID).map_or(0.0, |panel| {
        panel.get_bounding_client_rect().height().ceil()
    });
    request_show(height);
}

/// Has the host give the window the content `height` and show it.
fn request_show(height: f64) {
    #[derive(Serialize)]
    struct Args {
        height: f64,
    }
    leptos::task::spawn_local(async move {
        if let Err(error) = tauri::call::<()>(CMD_SCRIPT_DIALOG_SHOW, Args { height }).await {
            log_error(&error);
        }
    });
}

/// The prompt's text field, else OK.
fn focus_first(document: &web_sys::Document) {
    if let Some(field) = document
        .get_element_by_id(TEXT_ID)
        .and_then(|element| element.dyn_into::<web_sys::HtmlInputElement>().ok())
    {
        let _ = field.focus();
        field.select();
    } else if let Some(ok) = document
        .get_element_by_id(OK_ID)
        .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = ok.focus();
    }
}

fn send_answer(answer: ScriptDialogAnswer) {
    #[derive(Serialize)]
    struct Args {
        answer: ScriptDialogAnswer,
    }
    leptos::task::spawn_local(async move {
        if let Err(error) = tauri::call::<()>(CMD_SCRIPT_DIALOG_ANSWER, Args { answer }).await {
            log_error(&error);
        }
    });
}
