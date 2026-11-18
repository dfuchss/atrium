//! The state the backend sends the UI to render.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AccountSettings, AppSettings, CustomService};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// In sidebar order.
    pub accounts: Vec<AccountView>,
    pub active: Option<Uuid>,
    /// Services that can be added.
    pub services: Vec<ServiceInfo>,
    /// The user-defined services (also contained in `services`, with `custom: true`).
    pub custom_services: Vec<CustomService>,
    pub dialog_open: bool,
    /// Account menu "Settings…": the dialog shows this account's settings. Stays set until the
    /// dialog closes, so a snapshot that follows right after cannot hide it from the UI.
    pub edit_request: Option<EditRequest>,
    pub settings: AppSettings,
    pub app: AppInfo,
    /// Problems of the last load of `<config>/services/*.toml` (startup or reload).
    pub service_errors: Vec<String>,
}

/// A request to show one account's settings. `seq` grows with each request, so asking again for
/// the same account is a new request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditRequest {
    pub account: Uuid,
    pub seq: u32,
}

/// Operating system the host runs on; the UI words the settings after it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperatingSystem {
    #[default]
    MacOs,
    Linux,
    Windows,
}

/// Read-only information for the "About" section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    pub os: OperatingSystem,
    pub version: String,
    pub config_dir: String,
    /// Folder of the user's service definition files (`<config_dir>/services`).
    pub services_dir: String,
    pub log_file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountView {
    pub id: Uuid,
    pub service: String,
    pub service_name: String,
    /// CSS color of the service, e.g. "#4A154B".
    pub color: String,
    /// One or two letters shown in the sidebar icon.
    pub initials: String,
    /// Direct unread count (mentions, DMs).
    pub unread: u32,
    /// Something unread without a count (channel activity) -> show a dot.
    pub indirect: bool,
    /// Label of the service's instance field, if the service has one (e.g. "Instance").
    pub instance_label: Option<String>,
    /// Image for the sidebar icon (custom icon, else cached favicon) as `data:`/https URL;
    /// None = draw `initials` on `color`.
    pub icon: Option<String>,
    /// `icon` is a monochrome glyph (Simple Icons, white): draw it padded on `color` instead of
    /// filling the icon box.
    pub icon_glyph: bool,
    /// The view is currently unloaded (hibernated or disabled).
    pub hibernated: bool,
    pub settings: AccountSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    /// When set, the add dialog asks for a `instance` value with this label (e.g. "Instance").
    pub instance_label: Option<String>,
    /// Pre-filled value for the instance field.
    pub instance_default: Option<String>,
    /// User-defined (editable/removable in settings).
    pub custom: bool,
    /// Service default for camera/microphone/screen-capture (see `AccountSettings::allow_media`).
    pub allow_media: bool,
    /// Definition details, so a custom service can start from this one as a template.
    pub url: String,
    pub title_regex: Option<String>,
    pub user_agent: Option<String>,
}
