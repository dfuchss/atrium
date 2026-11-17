//! The swap helper. Atrium starts its own installed (already trusted) app again with
//! [`APPLY_UPDATE_ARGUMENT`] right before it exits (the bundle's executable on macOS, the
//! `AppImage` on Linux); that process waits until Atrium is gone, swaps the installation unless
//! another process still runs from it, records the result and, where the platform does that,
//! opens Atrium again. It never initialises Tauri.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::native::{SWAPS_IN_ONE_RENAME, process_alive, processes_running_from, relaunch};
use crate::store::{SwapResult, UpdaterFiles, open_private, write_private};
use crate::swap::{BundleFs, RealFs, SwapOutcome, swap};

/// First argument that turns the app executable into the swap helper.
pub const APPLY_UPDATE_ARGUMENT: &str = "--atrium-apply-update";
/// How long the helper waits for Atrium to exit before it gives up without touching anything.
const EXIT_TIMEOUT: Duration = Duration::from_secs(300);
const EXIT_POLL: Duration = Duration::from_millis(200);

/// Everything the helper needs, passed as arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperPlan {
    pub app_pid: u32,
    pub install: PathBuf,
    pub updates_dir: PathBuf,
    pub version: String,
}

impl HelperPlan {
    pub fn to_args(&self) -> Vec<OsString> {
        vec![
            APPLY_UPDATE_ARGUMENT.into(),
            self.app_pid.to_string().into(),
            self.install.clone().into(),
            self.updates_dir.clone().into(),
            self.version.clone().into(),
        ]
    }

    /// `None` when the process is not the helper (the normal app start); the program name is
    /// skipped.
    pub fn from_args(
        args: impl IntoIterator<Item = OsString>,
    ) -> Option<Result<HelperPlan, String>> {
        let mut args = args.into_iter().skip(1);
        if args.next()? != APPLY_UPDATE_ARGUMENT {
            return None;
        }
        let rest: Vec<OsString> = args.collect();
        let [pid, install, updates_dir, version] = rest.as_slice() else {
            return Some(Err(format!(
                "expected 4 helper arguments, got {}",
                rest.len()
            )));
        };
        let Some(app_pid) = pid.to_str().and_then(|pid| pid.parse().ok()) else {
            return Some(Err("bad process id".into()));
        };
        let (install, updates_dir) = (PathBuf::from(install), PathBuf::from(updates_dir));
        if !install.is_absolute() || !updates_dir.is_absolute() {
            return Some(Err("helper paths must be absolute".into()));
        }
        Some(Ok(HelperPlan {
            app_pid,
            install,
            updates_dir,
            version: version.to_string_lossy().into_owned(),
        }))
    }

    /// Starts the helper detached from Atrium, so it outlives the app; output goes nowhere,
    /// the helper writes its own log.
    pub fn spawn(&self, executable: &Path) -> std::io::Result<()> {
        detach(
            Command::new(executable)
                .args(self.to_args())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .spawn()
        .map(|_| ())
    }
}

/// Runs `command` in a process group of its own (no terminal), in `/`.
#[cfg(unix)]
fn detach(command: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt as _;

    command.current_dir("/").process_group(0)
}

/// Runs `command` without a console, in a process group of its own.
#[cfg(windows)]
fn detach(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt as _;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
}

/// Runs the helper when the process was started as one; returns its exit code then. Call it
/// first thing in `main`.
pub fn run_if_requested() -> Option<i32> {
    let plan = HelperPlan::from_args(std::env::args_os())?;
    Some(match plan {
        Ok(plan) => {
            let io = HelperIo {
                fs: &RealFs,
                app_alive: &process_alive,
                running_from: &processes_running_from,
                relaunch: &relaunch,
                exit_timeout: EXIT_TIMEOUT,
            };
            i32::from(!run(&plan, &io).success)
        }
        Err(_) => 2,
    })
}

/// The side effects of the helper, replaceable in tests.
pub(crate) struct HelperIo<'a> {
    pub(crate) fs: &'a dyn BundleFs,
    pub(crate) app_alive: &'a dyn Fn(u32) -> bool,
    /// Ids of the other processes running from the given installation.
    pub(crate) running_from: &'a dyn Fn(&Path) -> Result<Vec<u32>, String>,
    pub(crate) relaunch: &'a dyn Fn(&Path),
    pub(crate) exit_timeout: Duration,
}

/// Waits for Atrium to exit, swaps, writes the result file and opens Atrium again: the
/// update, the restored previous version, or the untouched install when Atrium did not exit
/// or another process still runs from the bundle (the update stays staged for next time).
pub(crate) fn run(plan: &HelperPlan, io: &HelperIo<'_>) -> SwapResult {
    let log = |message: &str| append_log(&plan.updates_dir, message);
    log(&format!(
        "waiting for Atrium (pid {}) to exit",
        plan.app_pid
    ));
    let outcome = if wait_for_exit(plan.app_pid, io) {
        swap_unless_in_use(&plan.install, io)
    } else {
        SwapOutcome::NotStarted("Atrium did not exit in time".into())
    };
    let result = SwapResult {
        success: outcome == SwapOutcome::Installed,
        version: plan.version.clone(),
        message: outcome.message(),
    };
    log(&format!("swap to {}: {}", plan.version, result.message));
    let json = serde_json::to_string(&result).unwrap_or_default();
    if let Err(error) = write_private(&UpdaterFiles::result_file(&plan.updates_dir), &json) {
        log(&format!("cannot write the result: {error}"));
    }
    // Even without a swap: a user who asked for a restart should not be left without Atrium
    // (on macOS `open` only activates an instance that still runs; Linux does nothing here).
    (io.relaunch)(&plan.install);
    result
}

/// Whether `pid` exited within the timeout.
fn wait_for_exit(pid: u32, io: &HelperIo<'_>) -> bool {
    let started = Instant::now();
    let mut exited = !(io.app_alive)(pid);
    while !exited && started.elapsed() < io.exit_timeout {
        std::thread::sleep(EXIT_POLL);
        exited = !(io.app_alive)(pid);
    }
    exited
}

/// Swaps only when no other process (a second instance started from the same bundle) runs
/// from the installation; the helper itself does, and is not counted.
fn swap_unless_in_use(install: &Path, io: &HelperIo<'_>) -> SwapOutcome {
    match (io.running_from)(install) {
        Ok(pids) if pids.is_empty() => swap(install, io.fs, SWAPS_IN_ONE_RENAME),
        Ok(pids) => SwapOutcome::NotStarted(format!(
            "Atrium still runs from the installation (pid {pids:?}); the update stays staged"
        )),
        Err(error) => SwapOutcome::NotStarted(format!(
            "cannot check for running copies of Atrium: {error}"
        )),
    }
}

fn append_log(updates_dir: &Path, message: &str) {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_secs());
    let _ = std::fs::create_dir_all(updates_dir);
    if let Ok(mut file) = open_private(UpdaterFiles::helper_log(updates_dir), true) {
        let _ = writeln!(file, "[{seconds}] helper: {message}");
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::native::tests::{fake_install, fake_staged, verify_staged};
    use crate::swap::new_update_path;

    fn plan(dir: &Path) -> HelperPlan {
        HelperPlan {
            app_pid: 4711,
            install: dir.join("Atrium.app"),
            updates_dir: dir.join("updates"),
            version: "0.2.0".into(),
        }
    }

    #[test]
    fn arguments_round_trip() {
        let plan = plan(&std::env::temp_dir());
        let mut args = vec![OsString::from(
            "/Applications/Atrium.app/Contents/MacOS/atrium",
        )];
        args.extend(plan.to_args());
        assert_eq!(HelperPlan::from_args(args), Some(Ok(plan)));
    }

    #[test]
    fn normal_starts_are_no_helper_and_bad_arguments_fail() {
        assert_eq!(HelperPlan::from_args(["atrium".into()]), None);
        assert_eq!(
            HelperPlan::from_args(["atrium".into(), "--other".into()]),
            None
        );
        let bad = [
            vec!["atrium", APPLY_UPDATE_ARGUMENT, "1"],
            vec!["atrium", APPLY_UPDATE_ARGUMENT, "x", "/a", "/b", "1"],
            vec!["atrium", APPLY_UPDATE_ARGUMENT, "1", "relative", "/b", "1"],
        ];
        for args in bad {
            let args = args.into_iter().map(OsString::from);
            assert!(matches!(HelperPlan::from_args(args), Some(Err(_))));
        }
    }

    /// The plan for an installation of 0.1.0 in `dir` with 0.2.0 staged next to it.
    fn staged_plan(dir: &Path) -> HelperPlan {
        let install = fake_install(dir);
        fake_staged(dir, &install, "0.2.0");
        HelperPlan {
            install,
            ..plan(dir)
        }
    }

    /// The real file system, an app that has exited, no other process and nothing relaunched.
    fn exited_app_io() -> HelperIo<'static> {
        HelperIo {
            fs: &RealFs,
            app_alive: &|_| false,
            running_from: &|_| Ok(Vec::new()),
            relaunch: &|_| {},
            exit_timeout: Duration::from_millis(300),
        }
    }

    #[test]
    fn swaps_after_exit_and_relaunches() {
        let dir = tempfile::tempdir().unwrap();
        let plan = staged_plan(dir.path());
        let relaunched = RefCell::new(Vec::new());
        let alive_checks = RefCell::new(0);
        let io = HelperIo {
            app_alive: &|_| {
                *alive_checks.borrow_mut() += 1;
                *alive_checks.borrow() < 3
            },
            relaunch: &|path| relaunched.borrow_mut().push(path.to_path_buf()),
            exit_timeout: Duration::from_secs(5),
            ..exited_app_io()
        };
        let result = run(&plan, &io);
        assert!(result.success, "{result:?}");
        verify_staged(&plan.install, "0.2.0");
        assert!(!new_update_path(&plan.install).exists());
        assert_eq!(*relaunched.borrow(), std::slice::from_ref(&plan.install));
        let written =
            std::fs::read_to_string(UpdaterFiles::result_file(&plan.updates_dir)).unwrap();
        assert_eq!(
            serde_json::from_str::<SwapResult>(&written).unwrap(),
            result
        );
        assert!(UpdaterFiles::helper_log(&plan.updates_dir).exists());
    }

    #[test]
    fn running_app_is_never_touched_but_opened_again() {
        let dir = tempfile::tempdir().unwrap();
        let plan = staged_plan(dir.path());
        let relaunched = RefCell::new(false);
        let io = HelperIo {
            app_alive: &|_| true,
            relaunch: &|_| *relaunched.borrow_mut() = true,
            ..exited_app_io()
        };
        let result = run(&plan, &io);
        assert!(!result.success);
        assert!(result.message.contains("did not exit"));
        verify_staged(&plan.install, "0.1.0");
        assert!(*relaunched.borrow());
    }

    #[test]
    fn another_running_copy_keeps_the_update_staged() {
        let dir = tempfile::tempdir().unwrap();
        let plan = staged_plan(dir.path());
        let asked = RefCell::new(Vec::new());
        let relaunched = RefCell::new(false);
        let io = HelperIo {
            running_from: &|dir| {
                asked.borrow_mut().push(dir.to_path_buf());
                Ok(vec![815])
            },
            relaunch: &|_| *relaunched.borrow_mut() = true,
            ..exited_app_io()
        };
        let result = run(&plan, &io);
        assert!(!result.success);
        assert!(result.message.contains("815"), "{}", result.message);
        assert_eq!(*asked.borrow(), vec![plan.install.clone()]);
        verify_staged(&plan.install, "0.1.0");
        verify_staged(&new_update_path(&plan.install), "0.2.0");
        assert!(*relaunched.borrow());
        let failing = HelperIo {
            running_from: &|_| Err("no ps".into()),
            ..io
        };
        assert!(!run(&plan, &failing).success);
        verify_staged(&new_update_path(&plan.install), "0.2.0");
    }

    #[test]
    fn own_process_is_alive() {
        assert!(process_alive(std::process::id()));
    }
}
