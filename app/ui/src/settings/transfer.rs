//! Export and import of the accounts, the settings and the services. The backend shows the file
//! dialogs and the result.

use leptos::prelude::*;
use protocol::{CMD_TRANSFER_COMMAND, Snapshot, TransferCommand, TransferScope};

use crate::widgets::Row;
use crate::{query, tauri};

fn send(command: TransferCommand) {
    if query::use_tauri() {
        leptos::task::spawn_local(tauri::send(CMD_TRANSFER_COMMAND, command));
    }
}

/// A button that sends `command`; `class` is `btn` or `btn danger`.
fn transfer_button(
    label: &'static str,
    class: &'static str,
    command: TransferCommand,
    disabled: Signal<bool>,
) -> impl IntoView {
    view! {
        <button class=class type="button" disabled=move || disabled.get() on:click=move |_| send(command)>{label}</button>
    }
}

#[component]
pub(super) fn TransferSection(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let has_accounts =
        Signal::derive(move || snapshot.with(|snapshot| !snapshot.accounts.is_empty()));
    let never = Signal::stored(false);
    view! {
        <h2>"Export and import"</h2>
        <div class="form">
            <Row label="Export" hint="Everything: the accounts, the settings and the services. Settings: the same without the accounts. Logins are not included.">
                <div class="actions">
                    {transfer_button("Export everything\u{2026}", "btn", TransferCommand::Export(TransferScope::Full), never)}
                    {transfer_button("Export settings\u{2026}", "btn", TransferCommand::Export(TransferScope::Settings), never)}
                </div>
            </Row>
            <Row label="Import" hint="Everything: only without accounts; Atrium restarts and you log in again. Settings: your accounts stay, custom services are updated or added.">
                <div class="actions">
                    {transfer_button("Import everything\u{2026}", "btn danger", TransferCommand::Import(TransferScope::Full), has_accounts)}
                    {transfer_button("Import settings\u{2026}", "btn danger", TransferCommand::Import(TransferScope::Settings), never)}
                </div>
            </Row>
        </div>
    }
}
