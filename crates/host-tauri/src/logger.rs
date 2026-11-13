//! Timestamped logging to stdout and `<app_log_dir>/atrium.log`.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager, Runtime};

const LOG_FILE_NAME: &str = "atrium.log";
const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Clone)]
pub(crate) struct Logger {
    file: Option<Arc<PathBuf>>,
}

impl Logger {
    /// Logs into the app's log folder (created if missing), or to stdout only.
    pub(crate) fn new<R: Runtime>(app: &AppHandle<R>) -> Logger {
        let file = app.path().app_log_dir().ok().and_then(|dir| {
            std::fs::create_dir_all(&dir).ok()?;
            Some(Arc::new(dir.join(LOG_FILE_NAME)))
        });
        Logger { file }
    }

    /// Logs into `<dir>/atrium.log`.
    pub(crate) fn in_dir(dir: &std::path::Path) -> Logger {
        Logger {
            file: Some(Arc::new(dir.join(LOG_FILE_NAME))),
        }
    }

    pub(crate) fn file(&self) -> Option<PathBuf> {
        self.file.as_deref().cloned()
    }

    #[expect(
        clippy::print_stdout,
        reason = "the console mirror of the log file, seen with `cargo tauri dev`"
    )]
    pub(crate) fn log(&self, message: &str) {
        let line = format!("{} {message}", timestamp());
        println!("{line}");
        if let Some(path) = &self.file
            && let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path.as_path())
        {
            // Nowhere left to report a failing log write.
            let _ = writeln!(file, "{line}");
        }
    }
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
