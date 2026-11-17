//! Windows: the release is a single executable, `atrium-windows-x86_64.exe`, that replaces the
//! running one in place (in any folder the user may write to, the file name is kept); the user
//! opens Atrium again after the swap. Windows cannot overwrite a running executable but can
//! rename it, so the swap moves it aside (`<install>.old-update`) first; the helper itself runs
//! from it, so the next start removes it.

use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::location::{InstallLocation, RunningAppImage, can_replace};
use crate::staging::place_next_to_install;
use crate::version::Version;

/// The user opens Atrium again after the swap.
pub(crate) const RELAUNCHES_AFTER_SWAP: bool = false;
/// A running executable cannot be replaced, only renamed: it is moved aside first.
pub(crate) const SWAPS_IN_ONE_RENAME: bool = false;

/// The file Cargo puts into every target folder; an executable below one is a build from
/// source, not the downloaded release.
const CARGO_TARGET_MARKER: &str = "CACHEDIR.TAG";

/// The release asset for a Windows machine of the given architecture
/// (`std::env::consts::ARCH`); the name is fixed by `.github/workflows/release.yml`. Only
/// x86-64 is released.
pub fn asset_name_for_arch(arch: &str) -> Option<&'static str> {
    (arch == "x86_64").then_some("atrium-windows-x86_64.exe")
}

/// Where the running executable is installed, and whether it may replace itself.
pub fn install_location(debug_build: bool) -> InstallLocation {
    match std::env::current_exe() {
        Ok(executable) => resolve(&executable, debug_build, &can_replace),
        Err(error) => InstallLocation::ManualOnly {
            reason: format!("cannot locate Atrium: {error}"),
        },
    }
}

/// Whether a Cargo target folder holds `executable`.
fn in_cargo_target(executable: &Path) -> bool {
    executable
        .ancestors()
        .skip(1)
        .any(|folder| folder.join(CARGO_TARGET_MARKER).is_file())
}

/// Only a release build outside Cargo's target folder updates itself, in place, and only when
/// `can_replace(executable)` says the user may rename and remove it.
fn resolve(
    executable: &Path,
    debug_build: bool,
    can_replace: &dyn Fn(&Path) -> bool,
) -> InstallLocation {
    let reason = if debug_build {
        "this is a development build"
    } else if in_cargo_target(executable) {
        "it runs from the build folder"
    } else if !can_replace(executable) {
        "the folder of Atrium is not writable for you"
    } else {
        return InstallLocation::SelfUpdatable {
            bundle: executable.to_path_buf(),
        };
    };
    InstallLocation::ManualOnly {
        reason: reason.into(),
    }
}

/// An executable is not an `AppImage`.
#[must_use]
pub fn running_appimage() -> Option<RunningAppImage> {
    None
}

/// Why a release without the file for this machine is only announced.
pub(crate) const NO_DOWNLOAD_REASON: &str = "the release has no download for this computer";

/// The sentence the About tab shows when updates have to be installed by hand.
pub(crate) fn manual_only_text(reason: &str) -> String {
    format!(
        "Atrium installs updates itself only when it runs as the downloaded program, but {reason}."
    )
}

/// Moves the verified download next to the install (`<install>.new-update`), which it returns.
/// Identity and version come from `SHA256SUMS` and the tag.
pub(crate) fn prepare_update(
    downloaded: &Path,
    _version: Version,
    install: &Path,
) -> Result<PathBuf, String> {
    place_next_to_install(downloaded, install)
}

/// The helper is the installed executable.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the platforms share this signature; macOS can fail"
)]
pub(crate) fn helper_executable(install: &Path) -> std::io::Result<PathBuf> {
    Ok(install.to_path_buf())
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

/// Always empty: renaming the executable while another instance runs from it is safe on
/// Windows; that instance keeps the old version, and its file is removed after it quit.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the platforms share this signature; macOS can fail"
)]
pub(crate) fn processes_running_from(_install: &Path) -> Result<Vec<u32>, String> {
    Ok(Vec::new())
}

/// Nothing to do: the user opens Atrium again after the swap.
pub(crate) fn relaunch(_install: &Path) {}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::swap::{new_update_path, partial_update_path};

    /// The release asset the flow tests use.
    pub(crate) const ASSET: &str = "atrium-windows-x86_64.exe";

    /// The release asset of Atrium in `version`: a file holding the version.
    pub(crate) fn fake_asset(_dir: &Path, version: &str) -> Vec<u8> {
        version.as_bytes().to_vec()
    }

    /// `Atrium.exe` in version 0.1.0 in `<dir>/Atrium`; returns its path.
    pub(crate) fn fake_install(dir: &Path) -> PathBuf {
        let folder = dir.join("Atrium");
        std::fs::create_dir_all(&folder).unwrap();
        let install = folder.join("Atrium.exe");
        std::fs::write(&install, fake_asset(dir, "0.1.0")).unwrap();
        install
    }

    /// Stages the executable of `version` next to `install`.
    pub(crate) fn fake_staged(dir: &Path, install: &Path, version: &str) {
        std::fs::write(new_update_path(install), fake_asset(dir, version)).unwrap();
    }

    /// Fails unless `path` is the executable of `version`.
    pub(crate) fn verify_staged(path: &Path, version: &str) {
        assert_eq!(std::fs::read(path).unwrap(), fake_asset(path, version));
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
        assert_eq!(
            resolve(&install, false, &|_| true),
            InstallLocation::SelfUpdatable { bundle: install }
        );
    }

    #[test]
    fn everything_else_only_notifies() {
        let dir = tempfile::tempdir().unwrap();
        let install = fake_install(dir.path());
        let target = dir.path().join("target");
        std::fs::create_dir_all(target.join("release")).unwrap();
        std::fs::write(target.join(CARGO_TARGET_MARKER), "Signature").unwrap();
        let built = target.join("release/atrium.exe");
        let cases = [
            (&install, true, true, "development"),
            (&built, false, true, "build folder"),
            (&install, false, false, "not writable"),
        ];
        for (executable, debug, replaceable, expected) in cases {
            let location = resolve(executable, debug, &|_| replaceable);
            assert!(
                location
                    .manual_reason()
                    .unwrap_or_default()
                    .contains(expected),
                "{location:?}"
            );
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
