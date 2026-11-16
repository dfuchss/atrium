//! What is specific to the platform, one item per concern; `macos` holds it for macOS,
//! everything else of the host is shared.
//!
//! - Web data: each account's views and windows keep their cookies and storage apart
//!   ([`isolate_view`], [`isolate_window`]); [`data_store_ids`] and [`remove_data_store`] list
//!   and delete what is kept per account.
//! - Child webviews: [`add_child`], [`build_service_view`], [`place`] and [`set_visible`] put
//!   the sidebar and the service views into the main window; [`force_reload`] reloads one
//!   bypassing the cache.
//! - JavaScript dialogs: [`install_dialogs`] and [`attach_dialogs`] show the dialogs of the
//!   service views and popups as decided by the `dialogs` policy; [`prepare_popup`]: what a
//!   popup's page may do before it is created.
//! - Crashed web processes: [`register_crash_hook`] has the core reload a service view whose
//!   web process died.
//! - OS integration: [`set_badge`], [`show_warning`], [`observe_screens_wake`],
//!   [`NOTIFICATION_SOUND`] and the tray icon.
//! - Settings: [`open_url_with`] opens links in the chosen browser.

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use macos::*;
