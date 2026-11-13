//! Wires the updater into the app: its Tauri commands, the status event to the sidebar UI and
//! the hand-over to the swap helper when the app exits.

use std::path::PathBuf;
use std::sync::Arc;

use protocol::{EVENT_UPDATE_STATUS, UI_WEBVIEW, UpdateCommand, UpdateStatus};
use tauri::{App, AppHandle, Emitter, Manager, State, Wry};
use tauri_plugin_opener::OpenerExt;
use updater::{
    GitHubSource, InstallLocation, LogSink, Updater, UpdaterAction, UpdaterConfig, Version,
};

/// The updater as Tauri state.
pub(crate) struct UpdaterState(Updater);

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]
pub(crate) fn update_command(
    app: AppHandle,
    state: State<'_, UpdaterState>,
    command: UpdateCommand,
) {
    match state.0.handle(&command) {
        UpdaterAction::None => {}
        UpdaterAction::Quit => host_tauri::graceful_quit(&app),
        UpdaterAction::OpenUrl(url) => {
            let _ = app.opener().open_url(url, None::<&str>);
        }
    }
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]
pub(crate) fn get_update_status(state: State<'_, UpdaterState>) -> UpdateStatus {
    state.0.status()
}

/// Where the running executable is installed, and whether it may replace itself.
fn install_location() -> InstallLocation {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match std::env::current_exe() {
        Ok(executable) => InstallLocation::resolve(
            &executable,
            home.as_deref(),
            cfg!(debug_assertions),
            &updater::can_replace,
        ),
        Err(error) => InstallLocation::ManualOnly {
            reason: format!("cannot locate Atrium: {error}"),
        },
    }
}

/// Creates the updater (state in `data_dir`), registers it with the app and starts it.
pub(crate) fn start(app: &App, data_dir: PathBuf, log: LogSink) -> std::io::Result<()> {
    let version = app.package_info().version.to_string();
    let config = UpdaterConfig {
        current_version: Version::parse(&version),
        location: install_location(),
        asset_name: updater::asset_name_for_arch(std::env::consts::ARCH),
        data_dir,
    };
    log(&format!("updater: installation {:?}", config.location));
    let handle = app.handle().clone();
    let emit = Arc::new(move |status: &UpdateStatus| {
        let _ = handle.emit_to(UI_WEBVIEW, EVENT_UPDATE_STATUS, status);
    });
    let updater = Updater::new(config, Box::new(GitHubSource::new(&version)), log, emit);
    updater.start()?;
    app.manage(UpdaterState(updater));
    Ok(())
}

/// Starts the swap helper if the user chose "Restart and update"; call when the app exits.
pub(crate) fn launch_pending_helper(app: &AppHandle<Wry>) {
    if let Some(state) = app.try_state::<UpdaterState>() {
        state.0.launch_pending_helper();
    }
}
