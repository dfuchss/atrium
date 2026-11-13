//! Where UI commands go: to Rust through Tauri, or into the local demo snapshot.

use leptos::prelude::*;
use protocol::{AppSettings, Snapshot, UiCommand};

use crate::{demo, query, tauri};

/// Setter of the snapshot, provided as context; the demo applies commands to it.
#[derive(Clone, Copy)]
pub(crate) struct Dispatch(pub(crate) WriteSignal<Snapshot>);

/// Sends `command` to Rust, or applies it locally when running as a browser demo.
pub(crate) fn send_command(command: UiCommand) {
    let Some(Dispatch(set_snapshot)) = use_context::<Dispatch>() else {
        return;
    };
    if query::use_tauri() {
        leptos::task::spawn_local(tauri::send(command));
    } else {
        set_snapshot.update(|snapshot| demo::apply_locally(snapshot, command));
    }
}

/// Applies `change` to a copy of the current settings and sends it.
pub(crate) fn update_settings(
    snapshot: ReadSignal<Snapshot>,
    change: impl FnOnce(&mut AppSettings),
) {
    let mut settings = snapshot.with_untracked(|current| current.settings.clone());
    change(&mut settings);
    send_command(UiCommand::UpdateSettings { settings });
}
