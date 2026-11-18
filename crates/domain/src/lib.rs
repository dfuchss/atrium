//! Pure core of the messenger aggregator (hexagonal architecture).
//!
//! Everything that happens enters as an [`Input`]; [`Core::handle`] returns the [`Effect`]s
//! the outside world must perform. Adapters implement the traits in [`ports`] and
//! [`runtime::apply`] maps effects onto them. The core never does IO itself.

mod catalog;
mod core;
mod dialogs;
mod model;
pub mod ports;
pub mod runtime;
mod security;
mod throttle;

pub use crate::catalog::{Catalog, ServiceDef, host_matches_any, webkit_user_agent};
pub use crate::core::{
    AccountMenu, AccountMenuAction, AccountMenuItem, Bounds, Core, DockBadge, Effect, Input,
    MenuAccount, MenuAction, MenuModel, NotificationReq, SimpleIconResult, ViewPolicy, ViewSpec,
    simple_icon_url,
};
pub use crate::dialogs::{DialogPolicy, DialogText, Withheld, dialog_origin_label};
pub use crate::model::{Account, Persisted, Settings, unused_store_ids};
pub use crate::security::{
    NewWindowGate, NewWindowRoute, nav_allowed, new_window_gate, new_window_route, redact_url,
    window_title,
};
pub use crate::throttle::{RateLimiter, WindowQuota, WindowRefusal};

/// Trimmed text; empty means none.
fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(String::from)
}

/// JavaScript injected into every service page: collects web notifications into a queue
/// (`window.__atrium`) that Rust drains by polling, as service pages get no Tauri IPC.
const NOTIFY_SHIM: &str = include_str!("../assets/notify-shim.js");
