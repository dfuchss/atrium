//! Wires the updater into the app: its Tauri commands, the status event to the sidebar UI and
//! the hand-over to the swap helper when the app exits.

#![expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use protocol::{EVENT_UPDATE_STATUS, UI_WEBVIEW, UpdateCommand, UpdateStatus};
use tauri::{App, AppHandle, Emitter, Manager, Wry};
use tauri_plugin_opener::OpenerExt;
use updater::{
    DirectorySource, GitHubSource, LogSink, ReleaseSource, Updater, UpdaterAction, UpdaterConfig,
    Version,
};

/// The updater as Tauri state; absent when it could not start.
pub(crate) struct UpdaterState(Updater);

#[tauri::command]
pub(crate) fn update_command(app: AppHandle, command: UpdateCommand) {
    let Some(state) = app.try_state::<UpdaterState>() else {
        return;
    };
    match state.0.handle(&command) {
        UpdaterAction::None => {}
        UpdaterAction::Quit => host_tauri::graceful_quit(&app),
        UpdaterAction::OpenUrl(url) => {
            let _ = app.opener().open_url(url, None::<&str>);
        }
    }
}

#[tauri::command]
pub(crate) fn get_update_status(app: AppHandle) -> Option<UpdateStatus> {
    app.try_state::<UpdaterState>()
        .map(|state| state.0.status())
}

/// Creates the updater (state in `data_dir`), registers it with the app and starts it.
/// `dev_release_dir` (debug builds only) replaces GitHub with a local release folder and lets
/// the debug build update itself, so the whole update can be tried by hand.
pub(crate) fn start(
    app: &App,
    data_dir: &Path,
    log: LogSink,
    dev_release_dir: Option<PathBuf>,
) -> std::io::Result<()> {
    let version = app.package_info().version.to_string();
    let debug_build = cfg!(debug_assertions) && dev_release_dir.is_none();
    let config = UpdaterConfig {
        current_version: Version::parse(&version),
        location: updater::install_location(debug_build),
        asset_name: updater::asset_name_for_arch(std::env::consts::ARCH),
        data_dir: data_dir.to_path_buf(),
    };
    log(&format!("updater: installation {:?}", config.location));
    let handle = app.handle().clone();
    let emit = Arc::new(move |status: &UpdateStatus| {
        let _ = handle.emit_to(UI_WEBVIEW, EVENT_UPDATE_STATUS, status);
    });
    let source: Box<dyn ReleaseSource> = match dev_release_dir {
        Some(dir) => {
            log(&format!("updater: releases from {}", dir.display()));
            Box::new(DirectorySource::new(dir))
        }
        None => Box::new(GitHubSource::new(&version)),
    };
    let updater = Updater::new(config, source, log, emit);
    updater.start()?;
    app.manage(UpdaterState(updater));
    Ok(())
}

/// Starts the swap helper if the user chose "Restart/Quit and update"; call when the app exits.
pub(crate) fn launch_pending_helper(app: &AppHandle<Wry>) {
    if let Some(state) = app.try_state::<UpdaterState>() {
        state.0.launch_pending_helper();
    }
}
