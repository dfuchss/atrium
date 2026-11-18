//! The desktop integration section of the General tab: install or remove the launcher entry of
//! the Linux `AppImage` or the Start menu shortcut on Windows. The backend decides whether there
//! is anything to offer.

use leptos::prelude::*;
use protocol::{
    CMD_GET_DESKTOP_ENTRY, CMD_INSTALL_DESKTOP_ENTRY, CMD_UNINSTALL_DESKTOP_ENTRY,
    DesktopEntryState,
};

use crate::widgets::{ErrorNote, Row};
use crate::{platform_texts, query, tauri};

/// What the section shows: the state (`None` while unknown or when there is nothing to offer)
/// and the message of the last failed request.
#[derive(Clone, Copy)]
struct DesktopEntry {
    state: RwSignal<Option<DesktopEntryState>>,
    error: RwSignal<Option<String>>,
}

impl DesktopEntry {
    /// Applies the result of a request: the new state, or the error with the state kept.
    fn show(self, result: Result<DesktopEntryState, String>) {
        match result {
            Ok(state) => {
                self.state.set(Some(state));
                self.error.set(None);
            }
            Err(message) => self.error.set(Some(message)),
        }
    }

    fn load(self) {
        if !query::use_tauri() {
            return;
        }
        leptos::task::spawn_local(async move {
            match tauri::call::<Option<DesktopEntryState>>(CMD_GET_DESKTOP_ENTRY, ()).await {
                Ok(state) => self.state.set(state),
                Err(message) => self.error.set(Some(message)),
            }
        });
    }

    fn request(self, command: &'static str) {
        leptos::task::spawn_local(async move {
            self.show(tauri::call(command, ()).await);
        });
    }
}

/// The state, for the `menu` Atrium installs itself in.
fn state_text(state: &DesktopEntryState, menu: &str) -> String {
    match state {
        DesktopEntryState::NotInstalled => format!("Atrium is not in your {menu} yet."),
        DesktopEntryState::Installed => format!("Atrium is in your {menu}."),
        DesktopEntryState::OtherPath { path } => {
            format!("The entry in your {menu} starts {path}, not this file.")
        }
    }
}

/// The section; nothing at all unless the backend offers desktop integration.
#[component]
pub(crate) fn DesktopIntegration() -> impl IntoView {
    let entry = DesktopEntry {
        state: RwSignal::new(None),
        error: RwSignal::new(None),
    };
    entry.load();
    let texts = platform_texts::texts();
    let is_state = move |wanted: fn(&DesktopEntryState) -> bool| {
        entry.state.with(|state| state.as_ref().is_some_and(wanted))
    };
    let install_label = move || {
        if is_state(|state| matches!(state, DesktopEntryState::OtherPath { .. })) {
            "Update"
        } else {
            "Install"
        }
    };
    view! {
        <Show when=move || entry.state.with(Option::is_some)>
            <h2 class="updates-title">"Desktop integration"</h2>
            <div class="form">
                <Row label=texts.app_menu_label>
                    <span>{move || entry.state.with(|state| state.as_ref().map(|state| state_text(state, texts.app_menu)))}</span>
                </Row>
                <Row label="">
                    <div class="update-actions">
                        <Show when=move || !is_state(|state| matches!(state, DesktopEntryState::Installed))>
                            <button class="btn" on:click=move |_| entry.request(CMD_INSTALL_DESKTOP_ENTRY)>{install_label}</button>
                        </Show>
                        <Show when=move || !is_state(|state| matches!(state, DesktopEntryState::NotInstalled))>
                            <button class="btn" on:click=move |_| entry.request(CMD_UNINSTALL_DESKTOP_ENTRY)>"Uninstall"</button>
                        </Show>
                    </div>
                </Row>
            </div>
        </Show>
        <ErrorNote message=entry.error />
    }
}
