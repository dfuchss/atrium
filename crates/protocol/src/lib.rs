//! Contract between the backend and the sidebar UI. Compiles to native and wasm32.
//!
//! - UI -> backend: Tauri command [`CMD_UI_COMMAND`] with argument `{ command: UiCommand }`.
//! - UI initial load: Tauri command [`CMD_GET_SNAPSHOT`] returns a [`Snapshot`].
//! - backend -> UI: event [`EVENT_SNAPSHOT`] (payload [`Snapshot`]) sent to the `ui` webview
//!   after every state change. The UI renders the snapshot; it keeps no state of its own
//!   besides form inputs.

mod commands;
mod desktop;
mod script_dialog;
mod services;
mod settings;
mod snapshot;
mod update;

pub use commands::*;
pub use desktop::*;
pub use script_dialog::*;
pub use services::*;
pub use settings::*;
pub use snapshot::*;
pub use update::*;

pub const CMD_UI_COMMAND: &str = "ui_command";
pub const CMD_GET_SNAPSHOT: &str = "get_snapshot";
/// Service definition editor commands (request/response, they bypass the core): arguments and
/// results are described on [`ServiceFile`] and [`ServiceProblem`].
pub const CMD_LIST_SERVICE_FILES: &str = "list_service_files";
pub const CMD_READ_SERVICE_FILE: &str = "read_service_file";
pub const CMD_NEW_SERVICE_TEMPLATE: &str = "new_service_template";
pub const CMD_VALIDATE_SERVICE: &str = "validate_service";
pub const CMD_SAVE_SERVICE_FILE: &str = "save_service_file";
pub const CMD_DELETE_SERVICE_FILE: &str = "delete_service_file";
pub const CMD_SET_SERVICE_FILE_ENABLED: &str = "set_service_file_enabled";
/// Export and import (argument `{ command: TransferCommand }`, bypasses the core).
pub const CMD_TRANSFER_COMMAND: &str = "transfer_command";
pub const EVENT_SNAPSHOT: &str = "snapshot";
/// Project page (repository) of Atrium.
pub const PROJECT_URL: &str = "https://github.com/dfuchss/atrium";
/// Label of the local sidebar webview.
pub const UI_WEBVIEW: &str = "ui";
