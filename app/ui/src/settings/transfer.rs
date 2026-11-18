//! Export and import of the settings, the accounts and the custom services.

use leptos::prelude::*;
use protocol::UiCommand;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

use crate::commands::send_command;
use crate::widgets::{ConfirmButton, ErrorNote, Row};

/// Reads the chosen file as text into `pending`; an empty or unreadable file sets `error`.
fn read_export(
    input: &web_sys::HtmlInputElement,
    pending: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
) {
    pending.set(None);
    error.set(None);
    let Some(file) = input.files().and_then(|files| files.get(0)) else {
        return;
    };
    let Ok(reader) = web_sys::FileReader::new() else {
        return;
    };
    let loaded_reader = reader.clone();
    let on_load = Closure::once_into_js(move || {
        match loaded_reader.result().ok().and_then(|text| text.as_string()) {
            Some(text) if !text.trim().is_empty() => pending.set(Some(text)),
            _ => error.set(Some("The file is empty.".into())),
        }
    });
    reader.set_onload(Some(on_load.unchecked_ref()));
    if reader.read_as_text(&file).is_err() {
        error.set(Some("Could not read the file.".into()));
    }
    input.set_value("");
}

#[component]
pub(super) fn TransferSection() -> impl IntoView {
    let pending: RwSignal<Option<String>> = RwSignal::new(None);
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let import = Callback::new(move |()| {
        if let Some(json) = pending.get_untracked() {
            pending.set(None);
            send_command(UiCommand::ImportSettings { json });
        }
    });
    view! {
        <h2>"Export and import"</h2>
        <div class="form">
            <Row label="Export" hint="Saves the settings, the added services and the custom services to a file in your Downloads folder. Logins are not included.">
                <button class="btn" type="button" on:click=move |_| send_command(UiCommand::ExportSettings)>"Export settings\u{2026}"</button>
            </Row>
            <Row label="Import" hint="Warning: importing overwrites all settings and removes all current services with their logins. The file is checked first; an invalid file changes nothing.">
                <div class="actions">
                    <label class="btn file-btn">
                        "Choose file\u{2026}"
                        <input type="file" accept=".json,application/json" hidden
                            on:change=move |event| read_export(&event_target::<web_sys::HtmlInputElement>(&event), pending, error) />
                    </label>
                    <Show when=move || pending.with(Option::is_some)>
                        <ConfirmButton label="Import file" confirm_label="Overwrite everything" on_confirm=import />
                    </Show>
                </div>
            </Row>
        </div>
        <ErrorNote message=error />
    }
}
