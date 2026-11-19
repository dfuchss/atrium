//! Windows: the release is a single executable, `atrium-windows-x86_64.exe`, that replaces the
//! running one in place (in any folder the user may write to, the file name is kept); the user
//! opens Atrium again after the swap. Windows cannot overwrite a running executable but can
//! rename it, so the swap moves it aside (`<install>.old-update`) first; the helper itself runs
//! from it, so the next start removes it.

use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::location::{self, InstallLocation};
use crate::native::single_file::resolve_executable;
pub(crate) use crate::native::single_file::{
    NO_DOWNLOAD_REASON, helper_executable, prepare_update, processes_running_from, relaunch,
};

/// The user opens Atrium again after the swap.
pub(crate) const RELAUNCHES_AFTER_SWAP: bool = false;
/// A running executable cannot be replaced, only renamed: it is moved aside first.
pub(crate) const SWAPS_IN_ONE_RENAME: bool = false;

/// The release asset for a Windows machine of the given architecture
/// (`std::env::consts::ARCH`); the name is fixed by `.github/workflows/release.yml`. Only
/// x86-64 is released.
pub fn asset_name_for_arch(arch: &str) -> Option<&'static str> {
    (arch == "x86_64").then_some("atrium-windows-x86_64.exe")
}

/// Where the running executable is installed, and whether it may replace itself.
pub fn install_location(debug_build: bool) -> InstallLocation {
    location::locate(
        debug_build,
        std::env::current_exe(),
        |executable, can_replace| resolve_executable(executable, can_replace),
    )
}

/// The sentence the About tab shows when updates have to be installed by hand.
pub(crate) fn manual_only_text(reason: &str) -> String {
    format!(
        "Atrium installs updates itself only when it runs as the downloaded program, but {reason}."
    )
}

/// Whether `tasklist` lists a process `pid`; when it cannot tell, the process counts as alive,
/// so no swap ever starts under a running Atrium.
pub(crate) fn process_alive(pid: u32) -> bool {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new(system_program("tasklist.exe"))
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            lists_process(&String::from_utf8_lossy(&output.stdout), pid)
        }
        _ => true,
    }
}

/// `program` in the Windows system folder, so no file of that name next to Atrium runs instead.
fn system_program(program: &str) -> PathBuf {
    let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Path::new(&windows).join("System32").join(program)
}

/// Whether the CSV rows of `tasklist /FO CSV` have `pid` in their second column, the process
/// id; without a match, `tasklist` prints a (translated) note instead.
fn lists_process(csv: &str, pid: u32) -> bool {
    let column = format!("\"{pid}\"");
    csv.lines()
        .any(|row| row.split(',').nth(1) == Some(column.as_str()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::native::single_file::fakes::fake_cargo_build;
    pub(crate) use crate::native::single_file::fakes::{fake_asset, fake_staged, verify_staged};
    use crate::swap::{new_update_path, partial_update_path};
    use crate::version::Version;

    /// The release asset the flow tests use.
    pub(crate) const ASSET: &str = "atrium-windows-x86_64.exe";

    /// `Atrium.exe` in version 0.1.0 in `<dir>/Atrium`; returns its path.
    pub(crate) fn fake_install(dir: &Path) -> PathBuf {
        let folder = dir.join("Atrium");
        std::fs::create_dir_all(&folder).unwrap();
        let install = folder.join("Atrium.exe");
        std::fs::write(&install, fake_asset(dir, "0.1.0")).unwrap();
        install
    }

    #[test]
    fn only_x86_64_has_an_asset() {
        assert_eq!(
            asset_name_for_arch("x86_64"),
            Some("atrium-windows-x86_64.exe")
        );
        assert_eq!(asset_name_for_arch("aarch64"), None);
    }

    #[test]
    fn a_release_build_in_a_writable_folder_updates_itself() {
        let dir = tempfile::tempdir().unwrap();
        let install = fake_install(dir.path());
        assert_eq!(resolve_executable(&install, &|_| true), Ok(install));
    }

    #[test]
    fn everything_else_only_notifies() {
        let dir = tempfile::tempdir().unwrap();
        let install = fake_install(dir.path());
        let built = fake_cargo_build(dir.path());
        let cases = [
            (&built, true, "build folder"),
            (&install, false, "not writable"),
        ];
        for (executable, replaceable, expected) in cases {
            let reason = resolve_executable(executable, &|_| replaceable).unwrap_err();
            assert!(reason.contains(expected), "{reason}");
        }
    }

    #[test]
    fn stages_a_copy_and_replaces_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let install = fake_install(dir.path());
        let downloaded = dir.path().join("download.exe");
        std::fs::write(&downloaded, fake_asset(dir.path(), "0.2.0")).unwrap();
        std::fs::write(new_update_path(&install), "old leftover").unwrap();
        std::fs::write(partial_update_path(&install), "broken copy").unwrap();
        let version = Version::parse("0.2.0").unwrap();

        let staged = prepare_update(&downloaded, version, &install).unwrap();
        assert_eq!(staged, new_update_path(&install));
        verify_staged(&staged, "0.2.0");
        assert!(!partial_update_path(&install).exists());
        assert!(!downloaded.exists());
        verify_staged(&install, "0.1.0");
    }

    #[test]
    fn finds_the_process_id_column() {
        let row = "\"atrium.exe\",\"4711\",\"Console\",\"1\",\"81.234 K\"\r\n";
        assert!(lists_process(row, 4711));
        assert!(!lists_process(row, 471));
        assert!(!lists_process(
            "INFO: No tasks are running which match the specified criteria.\r\n",
            4711
        ));
    }

    #[test]
    fn own_process_is_alive_and_an_exited_one_is_not() {
        assert!(process_alive(std::process::id()));
        let mut exited = Command::new(system_program("cmd.exe"))
            .args(["/C", "exit"])
            .spawn()
            .unwrap();
        exited.wait().unwrap();
        assert!(!process_alive(exited.id()));
    }
}
