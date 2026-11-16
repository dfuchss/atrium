//! What differs between the platforms Atrium runs on, one item per concern; `macos` and
//! `linux` export the same items (`linux` also `on_requested_size` and
//! `UNDECIDED_WINDOW_SIZE`, for `links`), everything else of the host is shared.
//!
//! - Web data: each account's views and windows keep their cookies and storage apart
//!   ([`isolate_view`], [`isolate_window`]); [`data_store_ids`] and [`remove_data_store`] list
//!   and delete what is kept per account.
//! - Child webviews: [`add_child`], [`build_service_view`], [`place`] and [`set_visible`] put
//!   the sidebar and the service views into the main window; [`force_reload`] reloads one
//!   bypassing the cache; [`POPUPS_RUN_OPENER_SCRIPTS`] tells whose user scripts a popup runs.
//! - JavaScript dialogs: [`install_dialogs`] and [`attach_dialogs`] show the dialogs of the
//!   service views and popups as decided by the `dialogs` policy; [`prepare_popup`]: what a
//!   popup's page may do before it is created.
//! - Crashed web processes: `register_crash_hook` (macOS) has the core reload a service
//!   view whose web process died.
//! - OS integration: [`set_app_id`], [`set_badge`], [`show_warning`], [`observe_screens_wake`],
//!   [`guard_single_instance`], [`NOTIFICATION_SOUND`], the tray icon and whether an app menu
//!   is shown.
//! - Settings: [`open_url_with`] opens links in the chosen browser.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
pub(crate) use linux::*;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("Atrium runs on macOS and Linux only");
