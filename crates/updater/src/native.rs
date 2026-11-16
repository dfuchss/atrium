//! What is specific to the platform Atrium updates itself on, one item per concern; `macos`
//! holds it for macOS, everything else of the updater is shared.
//!
//! - Release and installation: [`asset_name_for_arch`] names the release file for this
//!   machine, [`install_location`] decides whether the running installation may replace itself
//!   and [`manual_only_text`] explains to the user why it may not.
//! - Staging: [`prepare_update`] turns the verified download into `<install>.new-update`.
//! - Swap helper: it waits with [`process_alive`], refuses to swap while
//!   [`processes_running_from`] finds another running copy and finally calls [`relaunch`].

#[cfg(target_os = "macos")]
mod macos;

// `pub` only lets the crate root re-export the public items; the rest stays `pub(crate)`.
#[cfg(target_os = "macos")]
pub use macos::*;
