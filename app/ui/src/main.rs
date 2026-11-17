//! Sidebar and settings UI of Atrium (Leptos, client-side rendered in the `ui` webview), and
//! the windows of JavaScript dialogs on Windows (`script_dialog`).

// Leptos' `#[component]` generates `pub` props structs inside restricted modules.
#![allow(unreachable_pub)]

mod commands;
mod demo;
mod demo_files;
mod desktop;
mod glyphs;
mod platform_texts;
mod query;
mod script_dialog;
mod service_api;
mod settings;
mod sidebar;
mod tauri;
mod update;
mod widgets;

use leptos::prelude::*;
use protocol::{OperatingSystem, SCRIPT_DIALOG_QUERY, Snapshot, UiCommand};

use crate::commands::{Dispatch, send_command};
use crate::query::query_param;
use crate::script_dialog::ScriptDialogWindow;
use crate::settings::{SettingsDialog, Tab, UiState};
use crate::sidebar::Sidebar;

/// Loads the snapshot (from Rust, or the demo data) and keeps it current.
fn connect(set_snapshot: WriteSignal<Snapshot>, open_dialog: bool) {
    if query::use_tauri() {
        tauri::listen_snapshots(move |snapshot| set_snapshot.set(snapshot));
        leptos::task::spawn_local(async move {
            if let Some(snapshot) = tauri::get_snapshot().await {
                set_snapshot.set(snapshot);
                if open_dialog {
                    tauri::send(UiCommand::SetDialog { open: true }).await;
                }
            }
        });
    } else if query::is_demo() {
        let mut snapshot = demo::demo_snapshot();
        snapshot.dialog_open = open_dialog;
        set_snapshot.set(snapshot);
    }
}

/// `?dialog=<tab>&edit=<index>&addcustom=1&editservice=<file>` open the settings dialog in a given state, in the
/// demo and in the app alike (screenshots without clicks, see "Debug options" in the wiki).
fn initial_ui_state() -> (UiState, bool) {
    let dialog = query_param("dialog");
    let tab = dialog
        .as_deref()
        .and_then(Tab::from_id)
        .unwrap_or(Tab::Accounts);
    let state = UiState {
        tab: RwSignal::new(tab),
        focus_add: RwSignal::new(false),
        edit_index: RwSignal::new(query_param("edit").and_then(|index| index.parse().ok())),
        add_custom: RwSignal::new(query_param("addcustom").is_some_and(|value| value != "0")),
        edit_service: RwSignal::new(query_param("editservice").filter(|name| !name.is_empty())),
        edit_account: RwSignal::new(None),
    };
    (state, dialog.is_some())
}

/// CSS variables of the sidebar geometry, from the icon size setting.
fn layout_variables(snapshot: &Snapshot) -> String {
    let size = snapshot.settings.icon_size;
    format!(
        "--icon: {}px; --sidebar: {}px; --gap: {}px;",
        size.icon_px(),
        size.sidebar_width(),
        (size.icon_px() * 0.25).round()
    )
}

/// The root's classes: `os-<system>` lets the stylesheet adapt to the platform's web view.
fn root_class(snapshot: &Snapshot) -> &'static str {
    match snapshot.app.os {
        OperatingSystem::MacOs => "root os-macos",
        OperatingSystem::Linux => "root os-linux",
        OperatingSystem::Windows => "root os-windows",
    }
}

#[component]
fn App() -> impl IntoView {
    let (snapshot, set_snapshot) = signal(Snapshot::default());
    let (ui_state, open_dialog) = initial_ui_state();
    connect(set_snapshot, open_dialog);
    provide_context(Dispatch(set_snapshot));
    provide_context(ui_state);
    platform_texts::provide(snapshot);
    let (update_status, set_update_status) = signal(None::<protocol::UpdateStatus>);
    update::connect_updates(set_update_status);
    provide_context(update::UpdateStatusSignal(update_status));

    let on_key = window_event_listener(leptos::ev::keydown, move |event| {
        if event.key() == "Escape" && snapshot.with_untracked(|snapshot| snapshot.dialog_open) {
            send_command(UiCommand::SetDialog { open: false });
        }
    });
    on_cleanup(move || on_key.remove());

    // Only notifies when the dialog flag actually flips, not on every snapshot.
    let dialog_open = Memo::new(move |_| snapshot.with(|snapshot| snapshot.dialog_open));
    let variables = Memo::new(move |_| snapshot.with(layout_variables));
    let edit_account = Memo::new(move |_| snapshot.with(|snapshot| snapshot.edit_account));
    Effect::new(move || {
        if let Some(id) = edit_account.get() {
            ui_state.tab.set(Tab::Accounts);
            ui_state.edit_account.set(Some(id));
        }
    });
    view! {
        <div class=move || root_class(&snapshot.get()) style=move || variables.get()>
            <Sidebar snapshot=snapshot />
            <Show when=move || dialog_open.get()>
                <SettingsDialog snapshot=snapshot />
            </Show>
        </div>
    }
}

/// The sidebar, or in a dialog window (`?script-dialog`, Windows) the JavaScript dialog.
fn main() {
    if query_param(SCRIPT_DIALOG_QUERY).is_some() {
        mount_to_body(ScriptDialogWindow);
    } else {
        mount_to_body(App);
    }
}
