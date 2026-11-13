//! Pure core of the messenger aggregator (hexagonal architecture).
//!
//! Everything that happens enters as an [`Input`]; [`Core::handle`] returns the [`Effect`]s
//! the outside world must perform. Adapters implement the traits in [`ports`] and
//! [`runtime::apply`] maps effects onto them. The core never does IO itself.

pub mod catalog;
pub mod core;
pub mod model;
pub mod ports;
pub mod runtime;
pub mod security;

pub use crate::catalog::{Catalog, CatalogError, ServiceDef, UnreadRules};
pub use crate::catalog::{host_matches_any, normalize_app_hosts};
pub use crate::core::{
    Bounds, Core, DockBadge, Effect, ICON_SCRIPT, Input, MenuAccount, MenuAction, MenuModel,
    NotificationReq, ViewPolicy, ViewSpec, WAKE_GAP_MS, ZOOM_STEPS,
};
pub use crate::model::{Account, Persisted, Settings};
pub use crate::security::{nav_allowed, opener_url_allowed, window_title};

/// JavaScript injected into every service page: collects web notifications into a queue
/// (`window.__msgr`) that Rust drains by polling, as service pages get no Tauri IPC.
pub const NOTIFY_SHIM: &str = include_str!("../assets/notify-shim.js");
