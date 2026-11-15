//! Timestamped logging to stdout and `<app_log_dir>/atrium.log` (owner-only, rotated).
//!
//! Callers log URLs only through [`domain::redact_url`] and page titles only when title
//! logging is switched on (see `Core::set_log_page_titles`).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use domain::RateLimiter;
use tauri::{AppHandle, Manager, Runtime};
use uuid::Uuid;

use crate::private_file;

const LOG_FILE_NAME: &str = "atrium.log";
/// The previous log after a rotation (one generation is kept).
const ROTATED_LOG_FILE_NAME: &str = "atrium.log.1";
/// Size at which the log is rotated.
const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;
const SECONDS_PER_DAY: u64 = 86_400;
/// Navigation lines per account and window; single-page apps and ad frames navigate a lot.
const NAVIGATION_LINES_PER_WINDOW: usize = 20;
const NAVIGATION_WINDOW_MS: u64 = 10_000;

/// The log file and the lock that keeps rotation and appends of all threads in order.
struct LogFile {
    path: PathBuf,
    rotate_bytes: u64,
    write_lock: Mutex<()>,
}

impl LogFile {
    fn new(path: PathBuf, rotate_bytes: u64) -> LogFile {
        // Files written before the log became owner-only keep their mode on append.
        let _ = private_file::restrict(&path);
        LogFile {
            path,
            rotate_bytes,
            write_lock: Mutex::new(()),
        }
    }

    fn append(&self, line: &str) {
        let _serialized = self
            .write_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if std::fs::metadata(&self.path).is_ok_and(|meta| meta.len() >= self.rotate_bytes) {
            let _ = std::fs::rename(&self.path, self.path.with_file_name(ROTATED_LOG_FILE_NAME));
        }
        if let Ok(mut file) = private_file::open(&self.path, true) {
            // Nowhere left to report a failing log write.
            let _ = writeln!(file, "{line}");
        }
    }
}

#[derive(Clone)]
pub(crate) struct Logger {
    file: Option<Arc<LogFile>>,
    navigation_limit: Arc<Mutex<RateLimiter<Uuid>>>,
}

impl Logger {
    fn with_file(path: Option<PathBuf>) -> Logger {
        Logger {
            file: path.map(|path| Arc::new(LogFile::new(path, LOG_ROTATE_BYTES))),
            navigation_limit: Arc::new(Mutex::new(RateLimiter::new(
                NAVIGATION_LINES_PER_WINDOW,
                NAVIGATION_WINDOW_MS,
            ))),
        }
    }

    /// Logs into the app's log folder (created if missing), or to stdout only.
    pub(crate) fn new<R: Runtime>(app: &AppHandle<R>) -> Logger {
        let path = app.path().app_log_dir().ok().and_then(|dir| {
            std::fs::create_dir_all(&dir).ok()?;
            Some(dir.join(LOG_FILE_NAME))
        });
        Logger::with_file(path)
    }

    /// Logs into `<dir>/atrium.log`.
    pub(crate) fn in_dir(dir: &Path) -> Logger {
        Logger::with_file(Some(dir.join(LOG_FILE_NAME)))
    }

    pub(crate) fn file(&self) -> Option<PathBuf> {
        self.file.as_ref().map(|file| file.path.clone())
    }

    #[expect(
        clippy::print_stdout,
        reason = "the console mirror of the log file, seen with `cargo tauri dev`"
    )]
    pub(crate) fn log(&self, message: &str) {
        let line = format!("{} {message}", timestamp());
        println!("{line}");
        if let Some(file) = &self.file {
            file.append(&line);
        }
    }

    /// Logs a navigation line of `account` unless it exceeded its navigation line budget.
    pub(crate) fn log_navigation(&self, account: Uuid, message: &str) {
        let allowed = self
            .navigation_limit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .allow(account, now_ms());
        if allowed {
            self.log(message);
        }
    }
}

/// Wall clock in ms since the Unix epoch.
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| {
            u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX)
        })
}

/// `YYYY-MM-DD HH:MM:SS.mmmZ` (UTC) without a date crate.
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs();
    let (year, month, day) = civil_date(seconds / SECONDS_PER_DAY);
    let second_of_day = seconds % SECONDS_PER_DAY;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}.{:03}Z",
        second_of_day / 3600,
        second_of_day / 60 % 60,
        second_of_day % 60,
        now.subsec_millis()
    )
}

/// Year, month and day of the day `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_date(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468; // days since 0000-03-01
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(path).unwrap().permissions())
            & 0o777
    }

    #[test]
    fn log_file_rotates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE_NAME);
        let file = LogFile::new(path.clone(), 64);
        file.append(&"a".repeat(80));
        #[cfg(unix)]
        assert_eq!(mode(&path), private_file::PRIVATE_MODE);
        file.append("second");
        let rotated = dir.path().join(ROTATED_LOG_FILE_NAME);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second\n");
        assert!(
            std::fs::read_to_string(&rotated)
                .unwrap()
                .starts_with("aaa")
        );
        file.append("third");
        assert!(
            std::fs::read_to_string(&rotated)
                .unwrap()
                .starts_with("aaa")
        );
    }

    #[cfg(unix)]
    #[test]
    fn existing_log_becomes_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE_NAME);
        std::fs::write(&path, "old\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        LogFile::new(path.clone(), LOG_ROTATE_BYTES);
        assert_eq!(mode(&path), private_file::PRIVATE_MODE);
    }

    #[test]
    fn navigation_lines_are_limited_per_account() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::in_dir(dir.path());
        let (first, second) = (Uuid::from_u128(1), Uuid::from_u128(2));
        for index in 0..NAVIGATION_LINES_PER_WINDOW + 5 {
            logger.log_navigation(first, &format!("first {index}"));
        }
        logger.log_navigation(second, "second");
        let written = std::fs::read_to_string(logger.file().unwrap()).unwrap();
        assert_eq!(
            written.matches("first").count(),
            NAVIGATION_LINES_PER_WINDOW
        );
        assert_eq!(written.matches("second").count(), 1);
    }

    #[test]
    fn timestamp_has_fixed_shape() {
        let stamp = timestamp();
        assert_eq!(stamp.len(), "2026-11-13 12:00:00.000Z".len());
        assert!(stamp.starts_with("20"));
    }

    #[test]
    fn civil_date_of_known_days() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(59), (1970, 3, 1));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
        assert_eq!(civil_date(20_405), (2025, 11, 13));
        assert_eq!(civil_date(20_819), (2027, 1, 1));
    }
}
