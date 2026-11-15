//! The updates section of the About tab; the status also marks the settings button.

use leptos::prelude::*;
use protocol::{UpdateCommand, UpdateState, UpdateStatus};

use crate::widgets::{Row, Toggle};
use crate::{query, tauri};

/// The updater's status, provided as context. It stays at its default in the browser demo,
/// which has no updater.
#[derive(Clone, Copy)]
pub(crate) struct UpdateStatusSignal(pub(crate) ReadSignal<UpdateStatus>);

/// Loads the status from Rust and keeps it current.
pub(crate) fn connect_updates(set_status: WriteSignal<UpdateStatus>) {
    if !query::use_tauri() {
        return;
    }
    tauri::listen_update_status(move |status| set_status.set(status));
    leptos::task::spawn_local(async move {
        if let Some(status) = tauri::get_update_status().await {
            set_status.set(status);
        }
    });
}

fn send_update_command(command: UpdateCommand) {
    if query::use_tauri() {
        leptos::task::spawn_local(tauri::send_update(command));
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

#[component]
pub(crate) fn UpdateSection() -> impl IntoView {
    let UpdateStatusSignal(status) = expect_context::<UpdateStatusSignal>();
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
                        <button class="btn primary" on:click=move |_| send_update_command(UpdateCommand::RestartToUpdate)>"Restart and update"</button>
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
            <p class="note">"Atrium quits, replaces itself with the new version and starts again."</p>
        </Show>
        {move || manual_reason().map(|reason| view! {
            <p class="note">{format!("Atrium cannot install updates itself here: {reason}. Download new versions from the release page.")}</p>
        })}
        {move || last_failure().map(|failure| view! {
            <p class="note update-failure">{format!("The last update was not installed ({failure}).")}</p>
        })}
    }
}
