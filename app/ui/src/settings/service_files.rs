//! The services folder in the custom services tab: its files, enable/disable/delete, and the
//! editor for one file.

use leptos::prelude::*;
use leptos::task::spawn_local;
use protocol::{ServiceFile, Snapshot, UiCommand};

use super::UiState;
use super::service_editor::{EditTarget, ServiceEditor};
use crate::commands::send_command;
use crate::service_api;
use crate::widgets::{ConfirmButton, ErrorNote, Row};

/// Re-reads the folder listing into `files`.
fn refresh(files: RwSignal<Vec<ServiceFile>>) {
    spawn_local(async move { files.set(service_api::list().await) });
}

/// The user's TOML folder: its files, the editor and the problems of the last load.
#[component]
pub(super) fn ServiceFiles(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let ui = expect_context::<UiState>();
    let files: RwSignal<Vec<ServiceFile>> = RwSignal::new(vec![]);
    let editor: RwSignal<Option<EditTarget>> = RwSignal::new(None);
    let operation_error: RwSignal<Option<String>> = RwSignal::new(None);
    refresh(files);
    if let Some(name) = ui.edit_service.get_untracked() {
        ui.edit_service.set(None);
        open_service_file(name, editor, operation_error);
    }
    let close_editor = Callback::new(move |()| {
        editor.set(None);
        refresh(files);
    });
    view! {
        <h2>"Service definition files"</h2>
        <p class="note">"Each TOML file defines one service (same format as the built-in ones). A file with the same id as a built-in service overrides it. Saving a file applies it right away. Use {instance} in url or internal_hosts for the part that differs per account (e.g. the server host); instance_label sets the field's label and instance_default pre-fills it."</p>
        <div class="form">
            <Row label="Folder"><code>{move || snapshot.with(|snapshot| snapshot.app.services_dir.clone())}</code></Row>
            <Row label="">
                <div class="actions">
                    <button class="btn" on:click=move |_| send_command(UiCommand::OpenServicesFolder)>"Open folder"</button>
                    <button class="btn" on:click=move |_| send_command(UiCommand::ReloadServices)>"Reload"</button>
                </div>
            </Row>
        </div>
        {move || match editor.get() {
            Some(target) => view! { <ServiceEditor target=target on_close=close_editor /> }.into_any(),
            None => view! { <FileList files=files editor=editor operation_error=operation_error /> }.into_any(),
        }}
        {move || {
            let errors = snapshot.with(|snapshot| snapshot.service_errors.clone());
            (!errors.is_empty()).then(|| view! {
                <div class="note error">
                    <For each=move || errors.clone().into_iter().enumerate() key=|(index, error)| (*index, error.clone())
                        children=|(_, error)| view! { <div><code>{error}</code></div> } />
                </div>
            })
        }}
    }
}

#[component]
fn FileList(
    files: RwSignal<Vec<ServiceFile>>,
    editor: RwSignal<Option<EditTarget>>,
    operation_error: RwSignal<Option<String>>,
) -> impl IntoView {
    let new_file = move |_| {
        spawn_local(async move {
            editor.set(Some(EditTarget {
                name: None,
                text: service_api::template().await,
            }));
        });
    };
    view! {
        <Show when=move || !files.with(Vec::is_empty) fallback=|| view! { <p class="empty">"No definition files yet."</p> }>
            <section class="list">
                <For each=move || files.get() key=|file| (file.name.clone(), file.enabled)
                    children=move |file| view! { <FileRow file=file files=files editor=editor operation_error=operation_error /> } />
            </section>
        </Show>
        <ErrorNote message=operation_error />
        <div class="actions"><button class="btn primary" on:click=new_file>"New definition\u{2026}"</button></div>
    }
}

/// Opens the file `name` in the editor; on failure the error is shown instead.
fn open_service_file(
    name: String,
    editor: RwSignal<Option<EditTarget>>,
    error: RwSignal<Option<String>>,
) {
    spawn_local(async move {
        match service_api::read(&name).await {
            Ok(text) => editor.set(Some(EditTarget {
                name: Some(name),
                text,
            })),
            Err(message) => error.set(Some(message)),
        }
    });
}

#[component]
fn FileRow(
    file: ServiceFile,
    files: RwSignal<Vec<ServiceFile>>,
    editor: RwSignal<Option<EditTarget>>,
    operation_error: RwSignal<Option<String>>,
) -> impl IntoView {
    let (edited, toggled, deleted) = (file.name.clone(), file.name.clone(), file.name.clone());
    let enabled = file.enabled;
    let report = move |result: Result<(), String>| {
        operation_error.set(result.err());
        refresh(files);
    };
    let edit = move |_| open_service_file(edited.clone(), editor, operation_error);
    let toggle = move |_| {
        let name = toggled.clone();
        spawn_local(async move { report(service_api::set_enabled(&name, !enabled).await) });
    };
    let delete = move |()| {
        let name = deleted.clone();
        spawn_local(async move { report(service_api::delete(&name).await) });
    };
    view! {
        <div class="row" class:file-disabled=!enabled>
            <span class="names"><strong class="mono">{file.name.clone()}</strong></span>
            <button class="btn" on:click=edit>"Edit"</button>
            <button class="btn" on:click=toggle>{if enabled { "Disable" } else { "Enable" }}</button>
            <ConfirmButton label="Delete" confirm_label="Confirm delete" on_confirm=delete />
        </div>
    }
}
