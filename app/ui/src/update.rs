//! The updates section of the About tab; the status also marks the settings button.

use leptos::prelude::*;
use protocol::{CMD_UPDATE_COMMAND, EVENT_UPDATE_STATUS, UpdateCommand, UpdateState, UpdateStatus};

use crate::widgets::{ErrorNote, Row, Toggle};
use crate::{query, tauri};

/// The updater's status, provided as context. `None` while it is unknown or when this build has
/// no updater; the browser demo shows the default status.
#[derive(Clone, Copy)]
pub(crate) struct UpdateStatusSignal(pub(crate) ReadSignal<Option<UpdateStatus>>);

/// Loads the status from Rust and keeps it current.
pub(crate) fn connect_updates(set_status: WriteSignal<Option<UpdateStatus>>) {
    if !query::use_tauri() {
        set_status.set(Some(UpdateStatus::default()));
        return;
    }
    tauri::listen(EVENT_UPDATE_STATUS, move |status| {
        set_status.set(Some(status));
    });
    leptos::task::spawn_local(async move {
        if let Some(status) = tauri::get_update_status().await {
            set_status.set(Some(status));
        }
    });
}

fn send_update_command(command: UpdateCommand) {
    if query::use_tauri() {
        leptos::task::spawn_local(tauri::send(CMD_UPDATE_COMMAND, command));
    }
}

fn state_text(state: &UpdateState) -> String {
    match state {
        UpdateState::Idle => "Not checked yet.".into(),
        UpdateState::Checking => "Checking for updates\u{2026}".into(),
        UpdateState::UpToDate => "Atrium is up to date.".into(),
        UpdateState::Available { version } => format!("Atrium {version} is available."),
        UpdateState::Downloading {
            version,
            percent: Some(percent),
        } => format!("Downloading Atrium {version}\u{2026} {percent}\u{a0}%"),
        UpdateState::Downloading { version, .. } => format!("Downloading Atrium {version}\u{2026}"),
        UpdateState::Ready { version } => format!("Atrium {version} is ready to install."),
        UpdateState::Failed { message } => format!("Update failed: {message}"),
    }
}

/// The button that quits Atrium and swaps in the staged update.
fn apply_label(relaunches: bool) -> &'static str {
    if relaunches {
        "Restart and update"
    } else {
        "Quit and update"
    }
}

/// What the apply button does.
fn apply_note(relaunches: bool) -> &'static str {
    if relaunches {
        "Atrium quits, replaces itself with the new version and starts again."
    } else {
        "Atrium quits and replaces itself with the new version; open it again afterwards."
    }
}

/// The updates section; nothing at all in a build without an updater.
#[component]
pub(crate) fn UpdateSection() -> impl IntoView {
    let UpdateStatusSignal(status) = expect_context::<UpdateStatusSignal>();
    view! {
        <Show when=move || status.with(Option::is_some)>
            <UpdateDetails />
        </Show>
    }
}

#[component]
fn UpdateDetails() -> impl IntoView {
    let UpdateStatusSignal(status) = expect_context::<UpdateStatusSignal>();
    let status = Signal::derive(move || status.get().unwrap_or_default());
    let state = Memo::new(move |_| status.with(|status| status.state.clone()));
    let ready = move || matches!(state.get(), UpdateState::Ready { .. });
    let busy = move || {
        matches!(
            state.get(),
            UpdateState::Checking | UpdateState::Downloading { .. }
        )
    };
    let has_update = move || status.with(UpdateStatus::has_update);
    let auto_check = Signal::derive(move || status.with(|status| status.auto_check));
    let manual_reason = move || status.with(|status| status.manual_reason.clone());
    let relaunches = move || status.with(|status| status.relaunches);
    let last_failure = move || status.with(|status| status.last_failure.clone());
    view! {
        <h2 class="updates-title">"Updates"</h2>
        <div class="form">
            <Row label="Status">
                <span class="update-state" class:ready=ready>{move || state_text(&state.get())}</span>
            </Row>
            <Row label="">
                <div class="update-actions">
                    <Show when=ready>
                        <button class="btn primary" on:click=move |_| send_update_command(UpdateCommand::ApplyUpdate)>{move || apply_label(relaunches())}</button>
                    </Show>
                    <button class="btn" disabled=busy on:click=move |_| send_update_command(UpdateCommand::CheckNow)>"Check now"</button>
                    <Show when=has_update>
                        <button class="btn" on:click=move |_| send_update_command(UpdateCommand::OpenReleasePage)>"Release notes"</button>
                    </Show>
                </div>
            </Row>
            <Toggle label="Check automatically" hint="On startup and every six hours." checked=auto_check
                on_change=move |enabled| send_update_command(UpdateCommand::SetAutoCheck { enabled }) />
        </div>
        <Show when=ready>
            <p class="note">{move || apply_note(relaunches())}</p>
        </Show>
        {move || manual_reason().map(|reason| view! {
            <p class="note">{format!("{reason} Download new versions from the release page.")}</p>
        })}
        <ErrorNote message=Signal::derive(move || {
            last_failure().map(|failure| format!("The last update was not installed ({failure})."))
        }) />
    }
}
