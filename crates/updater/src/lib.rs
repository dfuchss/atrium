//! Self-update from GitHub Releases for macOS; on other systems new releases are only
//! announced.
//!
//! A check fetches the latest release, picks the zip for this Mac's architecture, verifies it
//! against the release's `SHA256SUMS`, extracts and checks the bundle under
//! `<app data>/updates/<tag>/` and moves it next to the installation (`<install>.new-update`).
//! The running bundle is never written: "Restart and update" quits Atrium, and a helper (the
//! app's own executable started with [`helper::APPLY_UPDATE_ARGUMENT`]) waits for the exit,
//! swaps the bundles, rolls back on failure and opens Atrium again.
//!
//! Releases are unsigned; the checksum file guards against broken or swapped downloads, not
//! against a compromised release.

mod archive;
mod bundle;
pub mod helper;
mod location;
mod quarantine;
mod release;
mod service;
mod source;
mod staging;
mod store;
mod swap;
mod version;

pub use location::InstallLocation;
#[cfg(unix)]
pub use location::can_replace;
pub use release::asset_name;
pub use service::{LogSink, StatusSink, Updater, UpdaterAction, UpdaterConfig};
pub use source::{Download, GitHubSource, ReleaseSource};
pub use version::Version;
