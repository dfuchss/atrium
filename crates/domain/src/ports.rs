//! Port traits implemented by adapters (`host-tauri`, `store-fs`, later `host-cef`).
//! Results that arrive asynchronously (poll results, clicks, title changes) are fed back to
//! the core as [`crate::Input`] by the adapter, not returned here.

use protocol::{AppSettings, Snapshot, Theme};
use uuid::Uuid;

use crate::core::{
    AccountMenu, Bounds, DockBadge, MenuModel, NotificationReq, ViewPolicy, ViewSpec,
};
use crate::model::Persisted;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortError(pub String);

impl std::fmt::Display for PortError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PortError {}

pub type PortResult<T = ()> = Result<T, PortError>;

/// The webview engine (Tauri/wry today, CEF later).
pub trait WebviewHost: Send + Sync {
    fn create(&self, spec: &ViewSpec, bounds: Bounds, visible: bool) -> PortResult;
    fn set_bounds(&self, account: Uuid, bounds: Bounds) -> PortResult;
    fn set_ui_bounds(&self, bounds: Bounds) -> PortResult;
    fn show(&self, account: Uuid) -> PortResult;
    fn hide(&self, account: Uuid) -> PortResult;
    fn destroy(&self, account: Uuid) -> PortResult;
    fn reload(&self, account: Uuid) -> PortResult;
    /// Reloads bypassing the cache (every resource revalidated with the server).
    fn force_reload(&self, account: Uuid) -> PortResult;
    fn eval(&self, account: Uuid, script: &str) -> PortResult;
    /// Evaluates `script`; the adapter later sends `Input::PollResult` with the raw callback string.
    fn poll(&self, account: Uuid, script: &str) -> PortResult;
    fn delete_profile(&self, store_id: [u8; 16]) -> PortResult;
    /// Link target and media permission of `account`'s views and popups. The adapter keeps
    /// the value and decides synchronously in its new-window and permission hooks.
    fn set_view_policy(&self, account: Uuid, policy: ViewPolicy) -> PortResult;
    /// Page zoom, 1.0 = 100 %.
    fn set_zoom(&self, account: Uuid, zoom: f64) -> PortResult;
    /// Deletes cookies/storage/cache of the view's data store, then reloads the view.
    fn clear_data(&self, account: Uuid) -> PortResult;
    /// URL currently shown in the view.
    fn current_url(&self, account: Uuid) -> PortResult<String>;
    /// Evaluates `script`; the adapter later sends `Input::IconFound` with the raw callback string.
    fn find_icon(&self, account: Uuid, script: &str) -> PortResult;
}

/// Native notifications; clicks come back as `Input::NotificationClicked`.
pub trait Notifier: Send + Sync {
    fn notify(&self, req: &NotificationReq) -> PortResult;
}

/// OS shell integration.
pub trait Platform: Send + Sync {
    fn set_dock_badge(&self, badge: DockBadge) -> PortResult;
    /// Opens `url` with the application `browser` (e.g. "Firefox"), or the system default.
    fn open_external(&self, url: &str, browser: Option<&str>) -> PortResult;
    /// Applies OS-level settings (launch at login, ...); must be idempotent.
    fn apply_system_settings(&self, settings: &AppSettings) -> PortResult;
    /// App-wide appearance: windows, menus, dialogs and the web content's `prefers-color-scheme`.
    fn apply_theme(&self, theme: Theme) -> PortResult;
    /// Creates `<config>/services/` (with a README and an example if it holds no definitions)
    /// and shows it in the file manager.
    fn open_services_folder(&self) -> PortResult;
    /// Saves `json` as a new file in the downloads folder and shows it in the file manager.
    fn export_settings(&self, json: &str) -> PortResult;
    /// Re-reads `<config>/services/*.toml` in the background; the adapter later sends
    /// `Input::ServicesLoaded`.
    fn load_services(&self) -> PortResult;
    fn focus_window(&self) -> PortResult;
    /// Downloads an icon in the background; the adapter later sends `Input::IconFetched`.
    fn fetch_icon(&self, account: Uuid, url: &str) -> PortResult;
    /// Downloads the Simple Icons glyph `slug` in the background; the adapter later sends
    /// `Input::SimpleIconFetched`.
    fn fetch_simple_icon(&self, account: Uuid, slug: &str) -> PortResult;
    /// Updates the app menu and the tray (menu bar) icon; must cope with unchanged parts.
    fn set_menus(&self, model: &MenuModel) -> PortResult;
    /// Pops up `menu` at the mouse pointer; a click is fed back as `Input::AccountMenu`.
    fn show_account_menu(&self, menu: &AccountMenu) -> PortResult;
    fn log(&self, message: &str);
}

/// Pushes state to the sidebar UI.
pub trait UiSink: Send + Sync {
    fn emit_snapshot(&self, snapshot: &Snapshot) -> PortResult;
}

/// Persistence of accounts and settings.
pub trait Store: Send + Sync {
    fn load(&self) -> PortResult<Persisted>;
    fn save(&self, data: &Persisted) -> PortResult;
}
