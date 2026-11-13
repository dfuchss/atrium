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
pub use crate::core::{
    Bounds, Core, DockBadge, Effect, ICON_SCRIPT, Input, MenuAccount, MenuAction, MenuModel, NotificationReq, ViewPolicy,
    ViewSpec, WAKE_GAP_MS, ZOOM_STEPS,
};
pub use crate::model::{Account, Persisted, Settings};
pub use crate::security::{nav_allowed, opener_url_allowed};

/// The JavaScript injected into service pages (plus an optional per-account style injector): collects web notifications into a queue
/// (`window.__msgr`) that Rust drains by polling. Service pages get no Tauri IPC capability.
pub const NOTIFY_SHIM: &str = include_str!("../assets/notify-shim.js");
