//! The updater's state machine: checks on startup and periodically, stages a newer release
//! and hands over to the swap helper when the user restarts.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use protocol::{UpdateCommand, UpdateState, UpdateStatus};

use crate::helper::HelperPlan;
use crate::location::InstallLocation;
use crate::native;
use crate::release::{RELEASES_PAGE, Release};
use crate::source::ReleaseSource;
use crate::staging::{StageRequest, remove_if_present, stage};
use crate::store::{SwapResult, UpdaterFiles, UpdaterSettings, write_atomically};
use crate::swap::{new_update_path, old_update_path, partial_update_path};
use crate::version::Version;

/// Time between automatic checks.
const CHECK_INTERVAL_MS: u64 = 6 * 60 * 60 * 1000;
/// The first check waits until startup work (loading all services) is done.
const STARTUP_DELAY: Duration = Duration::from_secs(30);
/// How often the background thread looks whether a check is due.
const TICK: Duration = Duration::from_mins(15);

pub type LogSink = Arc<dyn Fn(&str) + Send + Sync>;
pub type StatusSink = Arc<dyn Fn(&UpdateStatus) + Send + Sync>;

/// What the running app is.
pub struct UpdaterConfig {
    /// `None` turns the updater off (a version that is no plain release version).
    pub current_version: Option<Version>,
    pub location: InstallLocation,
    /// The release asset for this Mac's architecture.
    pub asset_name: Option<&'static str>,
    /// `<app data>`: `updater.json` and `updates/` live there.
    pub data_dir: PathBuf,
}

/// What the app has to do after an [`UpdateCommand`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdaterAction {
    None,
    /// Quit gracefully; the helper starts when the app exits ([`Updater::launch_pending_helper`]).
    Quit,
    OpenUrl(String),
}

/// What a check does with the latest release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckDecision {
    UpToDate,
    /// Newer, but this installation cannot install it itself; `reason` says why.
    ManualOnly {
        reason: String,
    },
    AlreadyStaged,
    Stage {
        install: PathBuf,
        asset_name: String,
    },
}

/// Never a downgrade: only strictly newer releases are offered.
pub(crate) fn decide(
    current: Version,
    release: Option<&Release>,
    config: &UpdaterConfig,
    staged: Option<Version>,
) -> CheckDecision {
    let Some(release) = release.filter(|release| release.version > current) else {
        return CheckDecision::UpToDate;
    };
    let bundle = match &config.location {
        InstallLocation::ManualOnly { reason } => {
            return CheckDecision::ManualOnly {
                reason: reason.clone(),
            };
        }
        InstallLocation::SelfUpdatable { bundle } => bundle,
    };
    let Some(asset_name) = config
        .asset_name
        .filter(|name| release.assets.contains_key(*name))
    else {
        return CheckDecision::ManualOnly {
            reason: native::NO_DOWNLOAD_REASON.into(),
        };
    };
    if staged == Some(release.version) {
        return CheckDecision::AlreadyStaged;
    }
    CheckDecision::Stage {
        install: bundle.clone(),
        asset_name: asset_name.to_string(),
    }
}

/// A staged update stays offered whatever a check reports.
fn ready_or(staged: Option<Version>, fallback: UpdateState) -> UpdateState {
    match staged {
        Some(version) => UpdateState::Ready {
            version: version.to_string(),
        },
        None => fallback,
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| {
            u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX)
        })
}

struct Shared {
    config: UpdaterConfig,
    source: Box<dyn ReleaseSource>,
    files: UpdaterFiles,
    log: LogSink,
    on_status: StatusSink,
    status: Mutex<UpdateStatus>,
    settings: Mutex<UpdaterSettings>,
    staged: Mutex<Option<Version>>,
    page_url: Mutex<String>,
    pending: Mutex<Option<HelperPlan>>,
    /// Held during a check (network I/O), so checks never overlap.
    checking: Mutex<()>,
}

/// Cheap to clone; all clones drive the same updater.
#[derive(Clone)]
pub struct Updater {
    shared: Arc<Shared>,
}

impl Updater {
    pub fn new(
        config: UpdaterConfig,
        source: Box<dyn ReleaseSource>,
        log: LogSink,
        on_status: StatusSink,
    ) -> Updater {
        let files = UpdaterFiles::new(config.data_dir.clone());
        let settings = files.load_settings();
        let status = UpdateStatus {
            state: UpdateState::Idle,
            auto_check: settings.auto_check,
            manual_reason: config
                .location
                .manual_reason()
                .map(native::manual_only_text),
            last_failure: None,
        };
        let shared = Shared {
            config,
            source,
            files,
            log,
            on_status,
            status: Mutex::new(status),
            settings: Mutex::new(settings),
            staged: Mutex::new(None),
            page_url: Mutex::new(RELEASES_PAGE.to_string()),
            pending: Mutex::new(None),
            checking: Mutex::new(()),
        };
        Updater {
            shared: Arc::new(shared),
        }
    }

    pub fn status(&self) -> UpdateStatus {
        locked(&self.shared.status).clone()
    }

    /// Cleans up after the previous run, then checks after a short delay and every few hours
    /// while automatic checks are on.
    pub fn start(&self) -> std::io::Result<()> {
        let updater = self.clone();
        std::thread::Builder::new()
            .name("updater".into())
            .spawn(move || {
                updater.clean_up_after_previous_run();
                if updater.shared.config.current_version.is_none() {
                    updater.log("updater off: the app version is no release version");
                    return;
                }
                std::thread::sleep(STARTUP_DELAY);
                loop {
                    updater.check_if_due(now_ms());
                    std::thread::sleep(TICK);
                }
            })
            .map(|_| ())
    }

    pub fn handle(&self, command: &UpdateCommand) -> UpdaterAction {
        match command {
            UpdateCommand::CheckNow => {
                let updater = self.clone();
                let spawned = std::thread::Builder::new()
                    .name("update-check".into())
                    .spawn(move || updater.check());
                if let Err(error) = spawned {
                    self.log(&format!("cannot start the update check: {error}"));
                }
                UpdaterAction::None
            }
            UpdateCommand::SetAutoCheck { enabled } => {
                let enabled = *enabled;
                self.change_settings(|settings| settings.auto_check = enabled);
                self.set_status(|status| status.auto_check = enabled);
                UpdaterAction::None
            }
            UpdateCommand::RestartToUpdate => self.prepare_restart(),
            UpdateCommand::OpenReleasePage => {
                UpdaterAction::OpenUrl(locked(&self.shared.page_url).clone())
            }
        }
    }

    /// Starts the prepared helper; call when the app exits. Returns whether it started. A
    /// helper that cannot start is reported on the next start.
    pub fn launch_pending_helper(&self) -> bool {
        let Some(plan) = locked(&self.shared.pending).take() else {
            return false;
        };
        match std::env::current_exe().and_then(|executable| plan.spawn(&executable)) {
            Ok(()) => {
                self.log(&format!("update helper started for {}", plan.version));
                true
            }
            Err(error) => {
                self.log(&format!("cannot start the update helper: {error}"));
                let result = SwapResult {
                    success: false,
                    version: plan.version,
                    message: format!("the update helper did not start: {error}"),
                };
                let json = serde_json::to_string(&result).unwrap_or_default();
                let _ = write_atomically(&UpdaterFiles::result_file(&plan.updates_dir), &json);
                false
            }
        }
    }

    fn log(&self, message: &str) {
        (self.shared.log)(&format!("updater: {message}"));
    }

    fn set_status(&self, change: impl FnOnce(&mut UpdateStatus)) {
        let status = {
            let mut status = locked(&self.shared.status);
            change(&mut status);
            status.clone()
        };
        (self.shared.on_status)(&status);
    }

    fn set_state(&self, state: UpdateState) {
        self.set_status(|status| status.state = state);
    }

    fn staged(&self) -> Option<Version> {
        *locked(&self.shared.staged)
    }

    fn set_staged(&self, version: Option<Version>) {
        *locked(&self.shared.staged) = version;
        self.change_settings(|settings| {
            settings.staged_version = version.map(|version| version.to_string());
        });
    }

    fn change_settings(&self, change: impl FnOnce(&mut UpdaterSettings)) {
        let mut settings = locked(&self.shared.settings);
        change(&mut settings);
        if let Err(error) = self.shared.files.save_settings(&settings) {
            self.log(&format!("cannot save the updater settings: {error}"));
        }
    }

    fn check_if_due(&self, now: u64) {
        let settings = locked(&self.shared.settings).clone();
        if settings.auto_check && now.saturating_sub(settings.last_check_ms) >= CHECK_INTERVAL_MS {
            self.check();
        }
    }
}

mod checking;

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests;
