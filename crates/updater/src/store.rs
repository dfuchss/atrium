//! The updater's files: `<app data>/updater.json` (settings and the staged release) and the
//! helper's `<app data>/updates/update-result.json`.

use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SETTINGS_FILE: &str = "updater.json";
const UPDATES_DIR: &str = "updates";
const RESULT_FILE: &str = "update-result.json";
const HELPER_LOG_FILE: &str = "helper.log";
/// The updater's files name versions and paths of this Mac: readable by the user only.
pub(crate) const PRIVATE_FILE_MODE: u32 = 0o600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UpdaterSettings {
    pub(crate) auto_check: bool,
    /// Last finished check, ms since the Unix epoch.
    pub(crate) last_check_ms: u64,
    /// Version of the update staged next to the install by an earlier run.
    pub(crate) staged_version: Option<String>,
}

impl Default for UpdaterSettings {
    fn default() -> Self {
        UpdaterSettings {
            auto_check: true,
            last_check_ms: 0,
            staged_version: None,
        }
    }
}

/// What the helper reports about a swap, read on the next start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SwapResult {
    pub(crate) success: bool,
    pub(crate) version: String,
    pub(crate) message: String,
}

/// Paths below the app data folder.
#[derive(Debug, Clone)]
pub(crate) struct UpdaterFiles {
    data_dir: PathBuf,
}

impl UpdaterFiles {
    pub(crate) fn new(data_dir: PathBuf) -> UpdaterFiles {
        UpdaterFiles { data_dir }
    }

    pub(crate) fn updates_dir(&self) -> PathBuf {
        self.data_dir.join(UPDATES_DIR)
    }

    pub(crate) fn result_file(updates_dir: &Path) -> PathBuf {
        updates_dir.join(RESULT_FILE)
    }

    pub(crate) fn helper_log(updates_dir: &Path) -> PathBuf {
        updates_dir.join(HELPER_LOG_FILE)
    }

    /// Defaults when the file is missing or unreadable.
    pub(crate) fn load_settings(&self) -> UpdaterSettings {
        std::fs::read_to_string(self.data_dir.join(SETTINGS_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub(crate) fn save_settings(&self, settings: &UpdaterSettings) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
        write_atomically(&self.data_dir.join(SETTINGS_FILE), &json)
    }

    /// Reads and removes the helper's result and log of the previous swap, if any.
    pub(crate) fn take_swap_report(&self) -> Option<(Option<SwapResult>, String)> {
        let updates = self.updates_dir();
        let result = std::fs::read_to_string(Self::result_file(&updates)).ok();
        let log = std::fs::read_to_string(Self::helper_log(&updates)).unwrap_or_default();
        let _ = std::fs::remove_file(Self::result_file(&updates));
        let _ = std::fs::remove_file(Self::helper_log(&updates));
        if result.is_none() && log.is_empty() {
            return None;
        }
        Some((
            result.and_then(|text| serde_json::from_str(&text).ok()),
            log,
        ))
    }
}

/// Writes `<path>.tmp` and renames it over `path`, so readers never see half a file.
pub(crate) fn write_atomically(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(".tmp");
    let mut file = open_private(&temporary, false)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path)
}

/// Opens `path` for writing (truncated, or appended to with `append`) with mode 0600, also
/// when it existed with another mode.
pub(crate) fn open_private(path: impl AsRef<Path>, append: bool) -> std::io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .mode(PRIVATE_FILE_MODE)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_owner_only_even_over_a_leftover() {
        let dir = tempfile::tempdir().unwrap();
        let leftover = dir.path().join(format!("{SETTINGS_FILE}.tmp"));
        std::fs::write(&leftover, "old").unwrap();
        std::fs::set_permissions(&leftover, std::fs::Permissions::from_mode(0o644)).unwrap();
        let files = UpdaterFiles::new(dir.path().to_path_buf());
        files.save_settings(&UpdaterSettings::default()).unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir.path().join(SETTINGS_FILE)), PRIVATE_FILE_MODE);
        assert_eq!(files.load_settings(), UpdaterSettings::default());
        let log = dir.path().join(HELPER_LOG_FILE);
        std::fs::write(&log, "first\n").unwrap();
        std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o644)).unwrap();
        open_private(&log, true)
            .unwrap()
            .write_all(b"second\n")
            .unwrap();
        assert_eq!(mode(&log), PRIVATE_FILE_MODE);
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "first\nsecond\n");
    }

    #[test]
    fn settings_round_trip_and_default() {
        let dir = tempfile::tempdir().unwrap();
        let files = UpdaterFiles::new(dir.path().to_path_buf());
        assert_eq!(files.load_settings(), UpdaterSettings::default());
        let settings = UpdaterSettings {
            auto_check: false,
            last_check_ms: 42,
            staged_version: Some("0.2.0".into()),
        };
        files.save_settings(&settings).unwrap();
        assert_eq!(files.load_settings(), settings);
        std::fs::write(dir.path().join(SETTINGS_FILE), r#"{"auto_check":false}"#).unwrap();
        assert!(!files.load_settings().auto_check);
        std::fs::write(dir.path().join(SETTINGS_FILE), "garbage").unwrap();
        assert_eq!(files.load_settings(), UpdaterSettings::default());
    }

    #[test]
    fn swap_report_is_taken_once() {
        let dir = tempfile::tempdir().unwrap();
        let files = UpdaterFiles::new(dir.path().to_path_buf());
        assert_eq!(files.take_swap_report(), None);
        let result = SwapResult {
            success: false,
            version: "0.2.0".into(),
            message: "no".into(),
        };
        let updates = files.updates_dir();
        write_atomically(
            &UpdaterFiles::result_file(&updates),
            &serde_json::to_string(&result).unwrap(),
        )
        .unwrap();
        std::fs::write(UpdaterFiles::helper_log(&updates), "line\n").unwrap();
        assert_eq!(
            files.take_swap_report(),
            Some((Some(result), "line\n".into()))
        );
        assert_eq!(files.take_swap_report(), None);
    }
}
