//! Tauri commands of the service definition editor. They work on the services folder directly
//! (request/response) instead of going through the core; only a successful save, delete or
//! toggle asks the core to reload the services.

#![expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]

use std::path::PathBuf;

use domain::Input;
use host_tauri::{Feed, services};
use protocol::{ServiceFile, ServiceProblem, UiCommand};
use tauri::State;

/// The services folder and the channel into the core.
pub(crate) struct ServiceFiles {
    pub(crate) dir: PathBuf,
    pub(crate) feed: Feed,
}

impl ServiceFiles {
    fn reload(&self) {
        (self.feed)(Input::Ui(UiCommand::ReloadServices));
    }

    /// Reloads the services if `result` succeeded.
    fn reload_after<T>(&self, result: Result<T, String>) -> Result<T, String> {
        if result.is_ok() {
            self.reload();
        }
        result
    }
}

#[tauri::command]
pub(crate) fn list_service_files(files: State<'_, ServiceFiles>) -> Vec<ServiceFile> {
    services::list_service_files(&files.dir)
}

#[tauri::command]
pub(crate) fn read_service_file(
    files: State<'_, ServiceFiles>,
    name: String,
) -> Result<String, String> {
    services::read_service_file(&files.dir, &name)
}

#[tauri::command]
pub(crate) fn new_service_template() -> String {
    services::new_service_template()
}

#[tauri::command]
pub(crate) fn validate_service(text: String) -> Vec<ServiceProblem> {
    domain::Catalog::diagnose(&text)
}

#[tauri::command]
pub(crate) fn save_service_file(
    files: State<'_, ServiceFiles>,
    name: String,
    text: String,
    create: bool,
) -> Result<Vec<ServiceProblem>, String> {
    let problems = services::save_service_file(&files.dir, &name, &text, create)?;
    if problems.is_empty() {
        files.reload();
    }
    Ok(problems)
}

#[tauri::command]
pub(crate) fn delete_service_file(
    files: State<'_, ServiceFiles>,
    name: String,
) -> Result<(), String> {
    files.reload_after(services::delete_service_file(&files.dir, &name))
}

#[tauri::command]
pub(crate) fn set_service_file_enabled(
    files: State<'_, ServiceFiles>,
    name: String,
    enabled: bool,
) -> Result<(), String> {
    files.reload_after(services::set_service_file_enabled(
        &files.dir, &name, enabled,
    ))
}
