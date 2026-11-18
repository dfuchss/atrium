//! Self-update from GitHub Releases for macOS, Linux and Windows.
//!
//! A check fetches the latest release, picks the asset for this machine (a zip of the app
//! bundle on macOS, an `AppImage` on Linux, an executable on Windows), verifies it against the release's `SHA256SUMS`
//! under `<app data>/updates/<tag>/`, checks it and moves it next to the installation
//! (`<install>.new-update`). The running installation is never written: "Restart and update"
//! (macOS) or "Quit and update" (Linux, Windows) quits Atrium, and a helper (the installed app
//! started with [`helper::APPLY_UPDATE_ARGUMENT`]) waits for the exit, swaps the installations
//! (on Linux in one atomic rename, on macOS and Windows with a rollback on failure) and, on
//! macOS, opens Atrium again. What differs per platform lives in `native`.
//!
//! Releases are unsigned; the checksum file guards against broken or swapped downloads, not
//! against a compromised release.

pub mod helper;
mod location;
mod native;
mod release;
mod service;
mod source;
mod staging;
mod store;
mod swap;
mod version;

pub use location::{InstallLocation, RunningAppImage};
#[cfg(not(windows))]
pub use native::running_appimage;
pub use native::{asset_name_for_arch, install_location};
pub use service::{LogSink, StatusSink, Updater, UpdaterAction, UpdaterConfig};
pub use source::{DirectorySource, Download, GitHubSource, ReleaseSource};
pub use version::Version;
