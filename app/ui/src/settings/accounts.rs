//! Accounts tab: the add form and the list of accounts with their actions.

use std::time::Duration;

use leptos::prelude::*;
use protocol::{ServiceInfo, Snapshot, UiCommand};
use uuid::Uuid;

use super::account_form::EditForm;
use super::{Tab, UiState};
use crate::commands::send_command;
use crate::glyphs::{ICON_BACK, ICON_DOWN, ICON_FORWARD, ICON_RELOAD, ICON_UP};
use crate::widgets::{
    ConfirmButton, Icon, Row, TextField, account_by_id, account_icon, focus_by_id,
    instance_field_label,
};

/// Value of the service option that jumps to the custom services tab.
const CUSTOM_OPTION: &str = "__custom__";
/// How long the "Added ..." note stays.
const ADDED_NOTE_DURATION: Duration = Duration::from_secs(3);

#[component]
pub(super) fn AccountsTab(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let ui = expect_context::<UiState>();
    // Which account is being edited; its draft lives inside `EditForm`.
    let initial_edit = ui.edit_index.get_untracked().and_then(|index| {
        snapshot.with_untracked(|snapshot| snapshot.accounts.get(index).map(|account| account.id))
    });
    ui.edit_index.set(None);
    let editing: RwSignal<Option<Uuid>> = RwSignal::new(initial_edit);
    Effect::new(move || {
        if let Some(id) = ui.edit_account.get() {
            editing.set(Some(id));
            ui.edit_account.set(None);
        }
    });
    let ids = Memo::new(move |_| {
        snapshot.with(|snapshot| {
            snapshot
                .accounts
                .iter()
                .map(|account| account.id)
                .collect::<Vec<_>>()
        })
    });
    view! {
        <h2>"Accounts"</h2>
        <h3>"Add account"</h3>
        <AddAccountForm snapshot=snapshot />
        <h3 class="spaced">"Your accounts"</h3>
        <section class="list">
            <Show
                when=move || !ids.with(Vec::is_empty)
                fallback=|| view! { <p class="empty">"No accounts yet. Use the \u{201C}Add account\u{201D} form above to add your first one."</p> }
            >
                <For
                    each=move || ids.get()
                    key=|id| *id
                    children=move |id| view! { <AccountRow snapshot=snapshot id=id editing=editing /> }
                />
            </Show>
        </section>
    }
}

/// Service, label and instance of a new account. Stays filled with the service's defaults
/// after adding, so several accounts can be added in a row.
#[component]
fn AddAccountForm(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let ui = expect_context::<UiState>();
    let service = RwSignal::new(String::new());
    let label = RwSignal::new(String::new());
    let instance = RwSignal::new(String::new());
    Effect::new(move |_| {
        if ui.focus_add.get() {
            ui.focus_add.set(false);
            focus_by_id("add-service");
        }
    });

    let services = Memo::new(move |_| snapshot.with(|snapshot| snapshot.services.clone()));
    let current = move || {
        let id = service.get();
        services.with(|list| {
            list.iter()
                .find(|candidate| candidate.id == id)
                .or(list.first())
                .cloned()
        })
    };
    // Reset the instance field only when the effective service changes.
    let current_id = Memo::new(move |_| current().map(|service| service.id));
    Effect::new(move |_| {
        if current_id.get().is_some() {
            instance.set(
                current()
                    .and_then(|service| service.instance_default)
                    .unwrap_or_default(),
            );
        }
    });

    let added: RwSignal<Option<String>> = RwSignal::new(None);
    let add = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let Some(service) = current() else { return };
        let added_label = submit_account(&service, label, instance);
        added.set(Some(added_label.clone()));
        set_timeout(
            move || {
                if added.get_untracked().as_deref() == Some(added_label.as_str()) {
                    added.set(None);
                }
            },
            ADDED_NOTE_DURATION,
        );
        focus_by_id("add-label");
    };

    view! {
        <form id="add-form" class="form" novalidate on:submit=add>
            <Row label="Service">
                <select
                    id="add-service"
                    prop:value=move || current().map_or_default(|service| service.id)
                    on:change=move |event| {
                        let value = event_target_value(&event);
                        if value == CUSTOM_OPTION {
                            // Restore the previous selection and jump to the custom services tab.
                            if let Some(id) = current().map(|service| service.id) {
                                event_target::<web_sys::HtmlSelectElement>(&event).set_value(&id);
                            }
                            ui.tab.set(Tab::Custom);
                        } else {
                            service.set(value);
                        }
                    }
                >
                    <For each=move || services.get() key=|service| service.id.clone() children=|service| {
                        let name = if service.custom { format!("{} (custom)", service.name) } else { service.name.clone() };
                        view! { <option value=service.id.clone()>{name}</option> }
                    } />
                    <option value=CUSTOM_OPTION>"+ Custom app or site\u{2026}"</option>
                </select>
            </Row>
            <Row label="Label">
                <TextField id="add-label" value=label plain=true placeholder=Signal::derive(move || Some(current().map_or_default(|service| service.name))) />
            </Row>
            {move || current().and_then(|service| service.instance_label).map(|instance_label| view! {
                <Row label=instance_field_label(instance_label)>
                    <TextField value=instance />
                </Row>
            })}
            // The note takes the empty label column beside the button, which is always there and
            // as wide as the labels: showing it never resizes or moves anything.
            <div class="frow">
                <span class="flabel added-note" title=move || added.get().map(|added_label| format!("Added {added_label}"))>
                    {move || added.get().map(|added_label| format!("Added {added_label}"))}
                </span>
                <div class="fctl">
                    <button class="btn primary" type="submit" disabled=move || current().is_none()>"Add"</button>
                </div>
            </div>
        </form>
    }
}

/// Sends `AddAccount` from the form fields and resets them; returns the label used.
fn submit_account(
    service: &ServiceInfo,
    label: RwSignal<String>,
    instance: RwSignal<String>,
) -> String {
    let instance_value = instance.get_untracked().trim().to_string();
    let instance_value =
        (service.instance_label.is_some() && !instance_value.is_empty()).then_some(instance_value);
    let label_value = label.get_untracked().trim().to_string();
    let label_value = if label_value.is_empty() {
        service.name.clone()
    } else {
        label_value
    };
    send_command(UiCommand::AddAccount {
        service: service.id.clone(),
        label: label_value.clone(),
        instance: instance_value,
    });
    label.set(String::new());
    instance.set(service.instance_default.clone().unwrap_or_default());
    label_value
}

#[component]
fn AccountRow(
    snapshot: ReadSignal<Snapshot>,
    id: Uuid,
    editing: RwSignal<Option<Uuid>>,
) -> impl IntoView {
    let account = account_by_id(snapshot, id);
    view! {
        {move || account.get().map(|account| {
            let is_editing = move || editing.get() == Some(id);
            let edited = account.clone();
            let global_hibernate = snapshot.with_untracked(|snapshot| snapshot.settings.hibernate_after_min);
            let global_after_wake = snapshot.with_untracked(|snapshot| snapshot.settings.after_wake);
            let media_default = snapshot.with_untracked(|snapshot| snapshot.services.iter().find(|service| service.id == account.service).is_some_and(|service| service.allow_media));
            view! {
                <div class="acct-item" class:open=is_editing>
                    <div class="row">
                        <button class="row-main" title="Edit account" on:click=move |_| {
                            editing.set(if editing.get_untracked() == Some(id) { None } else { Some(id) });
                        }>
                            {account_icon(&account, true)}
                            <span class="names"><strong>{account.service_name.clone()}</strong><span class="sub">{account.settings.label.clone()}</span></span>
                            {account.settings.muted.then(|| view! { <span class="sub">"\u{1F507}"</span> })}
                        </button>
                        <div class="btn-group">
                            <button class="btn icon-btn" title="Move up" on:click=move |_| send_command(UiCommand::Move { account: id, delta: -1 })><Icon svg=ICON_UP /></button>
                            <button class="btn icon-btn" title="Move down" on:click=move |_| send_command(UiCommand::Move { account: id, delta: 1 })><Icon svg=ICON_DOWN /></button>
                            <button class="btn icon-btn" title="Reload" on:click=move |_| send_command(UiCommand::Reload { account: id })><Icon svg=ICON_RELOAD /></button>
                        </div>
                        <ConfirmButton label="Remove" confirm_label="Confirm remove" on_confirm=move |()| send_command(UiCommand::RemoveAccount { account: id }) />
                    </div>
                    <AccountActions id=id enabled=account.settings.enabled />
                    {move || is_editing().then(|| view! { <EditForm account=edited.clone() editing=editing media_default=media_default global_hibernate=global_hibernate global_after_wake=global_after_wake /> })}
                </div>
            }
        })}
    }
}

/// Browser, history, sleep and data actions of one account.
#[component]
fn AccountActions(id: Uuid, enabled: bool) -> impl IntoView {
    view! {
        <div class="row actions-row">
            <button class="btn small" title="Open the current page in the browser" on:click=move |_| send_command(UiCommand::OpenInBrowser { account: id })>"Open in browser"</button>
            <div class="btn-group">
                <button class="btn small icon-btn" title="Back" on:click=move |_| send_command(UiCommand::GoBack { account: id })><Icon svg=ICON_BACK /></button>
                <button class="btn small icon-btn" title="Forward" on:click=move |_| send_command(UiCommand::GoForward { account: id })><Icon svg=ICON_FORWARD /></button>
            </div>
            <button class="btn small" title="Unload the view to save memory" disabled=!enabled on:click=move |_| send_command(UiCommand::Hibernate { account: id })>"Sleep now"</button>
            <ConfirmButton small=true label="Clear website data\u{2026}" confirm_label="Confirm: log out" title="Logs you out of this service" on_confirm=move |()| send_command(UiCommand::ClearData { account: id }) />
        </div>
    }
}
