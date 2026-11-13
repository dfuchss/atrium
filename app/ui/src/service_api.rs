//! The service definition editor's backend calls: Tauri commands in the app, an in-memory
//! folder in the browser demo.

use protocol::{
    CMD_DELETE_SERVICE_FILE, CMD_LIST_SERVICE_FILES, CMD_NEW_SERVICE_TEMPLATE,
    CMD_READ_SERVICE_FILE, CMD_SAVE_SERVICE_FILE, CMD_SET_SERVICE_FILE_ENABLED,
    CMD_VALIDATE_SERVICE, ServiceFile, ServiceProblem,
};
use serde::Serialize;

use crate::{demo_files, query, tauri};

/// Sample definition of the demo's "New".
const DEMO_TEMPLATE: &str = "id = \"example\"\nname = \"Example Chat\"\ncolor = \"#3B82F6\"\nurl = \"https://{instance}/\"\n";

#[derive(Serialize)]
struct NameArgs<'a> {
    name: &'a str,
}

#[derive(Serialize)]
struct TextArgs<'a> {
    text: &'a str,
}

#[derive(Serialize)]
struct SaveArgs<'a> {
    name: &'a str,
    text: &'a str,
    create: bool,
}

#[derive(Serialize)]
struct EnabledArgs<'a> {
    name: &'a str,
    enabled: bool,
}

pub(crate) async fn list() -> Vec<ServiceFile> {
    if query::use_tauri() {
        tauri::call(CMD_LIST_SERVICE_FILES, ())
            .await
            .unwrap_or_default()
    } else {
        demo_files::list()
    }
}

pub(crate) async fn read(name: &str) -> Result<String, String> {
    if query::use_tauri() {
        tauri::call(CMD_READ_SERVICE_FILE, NameArgs { name }).await
    } else {
        demo_files::read(name)
    }
}

pub(crate) async fn template() -> String {
    if query::use_tauri() {
        tauri::call(CMD_NEW_SERVICE_TEMPLATE, ())
            .await
            .unwrap_or_default()
    } else {
        DEMO_TEMPLATE.into()
    }
}

/// A failing call is reported as a problem without a position, so the editor shows it.
pub(crate) async fn validate(text: &str) -> Vec<ServiceProblem> {
    if !query::use_tauri() {
        return demo_files::validate(text);
    }
    tauri::call(CMD_VALIDATE_SERVICE, TextArgs { text })
        .await
        .unwrap_or_else(|message| {
            vec![ServiceProblem {
                message,
                line: None,
                column: None,
            }]
        })
}

/// `Ok` with problems: the text was refused. `create` refuses to replace an existing file.
pub(crate) async fn save(
    name: &str,
    text: &str,
    create: bool,
) -> Result<Vec<ServiceProblem>, String> {
    if query::use_tauri() {
        tauri::call(CMD_SAVE_SERVICE_FILE, SaveArgs { name, text, create }).await
    } else {
        demo_files::save(name, text, create)
    }
}

pub(crate) async fn delete(name: &str) -> Result<(), String> {
    if query::use_tauri() {
        tauri::call(CMD_DELETE_SERVICE_FILE, NameArgs { name }).await
    } else {
        demo_files::delete(name);
        Ok(())
    }
}

pub(crate) async fn set_enabled(name: &str, enabled: bool) -> Result<(), String> {
    if query::use_tauri() {
        tauri::call(CMD_SET_SERVICE_FILE_ENABLED, EnabledArgs { name, enabled }).await
    } else {
        demo_files::set_enabled(name, enabled);
        Ok(())
    }
}
