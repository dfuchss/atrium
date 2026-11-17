//! What differs between the platforms Atrium updates itself on, one item per concern; `macos`,
//! `linux` and `windows` export the same items, everything else of the updater is shared.
//!
//! - Release and installation: [`asset_name_for_arch`] names the release file for this
//!   machine, [`install_location`] decides whether the running installation may replace itself
//!   and [`manual_only_text`] explains to the user why it may not; [`running_appimage`] names
//!   the `AppImage` Atrium runs from (never on macOS).
//! - Staging: [`prepare_update`] turns the verified download into `<install>.new-update`.
//! - Swap helper: [`helper_executable`] is started when the app exits; it waits with
//!   [`process_alive`], refuses while [`processes_running_from`] finds another running copy,
//!   swaps in one rename where [`SWAPS_IN_ONE_RENAME`] says so and finally calls [`relaunch`],
//!   which starts Atrium again where [`RELAUNCHES_AFTER_SWAP`] says so.
//! - Tests: `tests` provides the release asset name and fakes of the installation and the asset.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

// `pub` only lets the crate root re-export the public items; the rest stays `pub(crate)`.
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
compile_error!("Atrium updates itself on macOS, Linux and Windows only");
