//! Tauri command of the export and import: it shows the file dialogs and the result and works on
//! the files directly; the imported state goes through the core.

use domain::{Input, ServiceDef};
use protocol::{TransferCommand, TransferScope};
use store_fs::{FsStore, services, transfer};
use tauri::{Manager, Window, Wry};
use tauri_plugin_dialog::{DialogExt as _, FilePath, MessageDialogBuilder, MessageDialogKind};

use crate::service_files::ServiceFiles;

#[tauri::command]
pub(crate) fn transfer_command(window: Window, command: TransferCommand) {
    let dialog = window
        .dialog()
        .file()
        .add_filter("Atrium export", &["json"])
        .set_parent(&window);
    match command {
        TransferCommand::Export(scope) => {
            dialog
                .set_file_name("atrium-export.json")
                .save_file(move |chosen| {
                    if let Some(chosen) = chosen {
                        report(&window, "Export", export(&window, scope, chosen));
                    }
                });
        }
        TransferCommand::Import(scope) => {
            dialog.pick_file(move |chosen| {
                if let Some(chosen) = chosen {
                    report(&window, "Import", import(&window, scope, chosen));
                }
            });
        }
    }
}

fn export(window: &Window, scope: TransferScope, chosen: FilePath) -> Result<(), String> {
    let path = chosen.into_path().map_err(|error| error.to_string())?;
    let app = window.app_handle();
    let services_dir = &app.state::<ServiceFiles>().dir;
    transfer::write_export(&path, &app.state::<FsStore>(), services_dir, scope)
}

fn import(window: &Window, scope: TransferScope, chosen: FilePath) -> Result<(), String> {
    let path = chosen.into_path().map_err(|error| error.to_string())?;
    let app = window.app_handle();
    let files = app.state::<ServiceFiles>();
    let (saved, skipped) = transfer::import(&path, &app.state::<FsStore>(), &files.dir, scope)?;
    if scope == TransferScope::Full {
        (files.feed)(Input::ImportEverything(saved));
        return Ok(());
    }
    let mut text = "Import successful.".to_string();
    append_list(
        &mut text,
        "These service files were not imported:",
        &skipped,
    );
    let rejected: Vec<String> = saved
        .custom_services
        .iter()
        .filter_map(|service| ServiceDef::from_custom(service).err())
        .collect();
    append_list(
        &mut text,
        "These custom services were not imported:",
        &rejected,
    );
    let (defs, errors) = services::read_user_services(&files.dir);
    (files.feed)(Input::ServicesLoaded { defs, errors });
    (files.feed)(Input::ImportSettings(saved));
    message(window, MessageDialogKind::Info, &text).show(|_| {});
    Ok(())
}

/// Appends `items` under `heading` to `text`, if there are any.
fn append_list(text: &mut String, heading: &str, items: &[String]) {
    if !items.is_empty() {
        text.push_str("\n\n");
        text.push_str(heading);
        text.push('\n');
        text.push_str(&items.join("\n"));
    }
}

/// Shows the error of `result`, if any.
fn report(window: &Window, action: &str, result: Result<(), String>) {
    if let Err(error) = result {
        let text = format!("{action} failed: {error}");
        message(window, MessageDialogKind::Error, &text).show(|_| {});
    }
}

fn message(window: &Window, kind: MessageDialogKind, text: &str) -> MessageDialogBuilder<Wry> {
    window.dialog().message(text).title("Atrium").kind(kind)
}
