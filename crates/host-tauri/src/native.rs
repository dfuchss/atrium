//! What differs between the platforms Atrium runs on, one item per concern; `macos`, `linux`
//! and `windows` export the same items (`linux` also `on_requested_size` and
//! `UNDECIDED_WINDOW_SIZE`, for `links`), everything else of the host is shared. What the
//! platforms other than macOS share lives here too: the per-account web data folders
//! (`web_data`) and the single-instance guard (`single_instance`); macOS and Windows share the
//! child webviews Tauri lays out by itself (`child_views`).
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
//! - Crashed web processes: [`register_crash_hook`] (macOS, for every webview) or
//!   [`observe_web_process_termination`] (Linux, per service view) has the core reload a service
//!   view whose web process died.
//! - User agents: [`user_agent_platform`] for the presets, [`NEW_ACCOUNT_USER_AGENT`] for new
//!   accounts.
//! - OS integration: [`set_app_id`], [`set_badge`], [`show_warning`], [`observe_resume`],
//!   [`guard_single_instance`], [`NOTIFICATION_SOUND`], the tray icon and whether an app menu
//!   is shown; [`Shortcuts`] where there is no app menu to carry them; [`announce_new_icons`]
//!   after the desktop entry's icons changed.
//! - Settings: [`open_url_with`] opens links in the chosen browser, [`installed_browsers`]
//!   lists the browsers to choose from; [`clean_launch_environment`] removes what the launcher
//!   set that would override the theme.

#[cfg(not(target_os = "linux"))]
mod child_views;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "linux", windows))]
mod single_instance;
#[cfg(not(target_os = "macos"))]
mod web_data;
#[cfg(windows)]
mod windows;

#[cfg(not(target_os = "linux"))]
pub(crate) use child_views::{place, set_visible};
#[cfg(target_os = "linux")]
pub(crate) use linux::*;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;
#[cfg(any(target_os = "linux", windows))]
pub(crate) use single_instance::guard_single_instance;
#[cfg(not(target_os = "macos"))]
pub(crate) use web_data::{data_store_ids, isolate_view, isolate_window, remove_data_store};
#[cfg(windows)]
pub(crate) use windows::*;

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
compile_error!("Atrium runs on macOS, Linux and Windows only");
