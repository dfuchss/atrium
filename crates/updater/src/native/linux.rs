//! Linux: the release is an `AppImage` that replaces the running one in place (in any folder the
//! user may write to, the file name is kept); the user opens Atrium again after the swap.

use std::path::{Path, PathBuf};

use fs_util::appimage::{AppImageEnv, RunningAppImage};

use crate::location::{self, InstallLocation};
pub(crate) use crate::native::single_file::{
    NO_DOWNLOAD_REASON, helper_executable, prepare_update, processes_running_from, relaunch,
};

/// The user opens Atrium again after the swap.
pub(crate) const RELAUNCHES_AFTER_SWAP: bool = false;
/// The `AppImage` is one file, which one rename replaces atomically.
pub(crate) const SWAPS_IN_ONE_RENAME: bool = true;

/// The release asset for a Linux machine of the given architecture
/// (`std::env::consts::ARCH`); the names are fixed by `.github/workflows/release.yml`. A
/// release without the file for this machine is only announced.
pub fn asset_name_for_arch(arch: &str) -> Option<&'static str> {
    match arch {
        "x86_64" => Some("atrium-linux-x86_64.AppImage"),
        "aarch64" => Some("atrium-linux-aarch64.AppImage"),
        _ => None,
    }
}

/// Where the running executable is installed, and whether it may replace itself.
pub fn install_location(debug_build: bool) -> InstallLocation {
    location::locate(debug_build, AppImageEnv::from_env(), resolve)
}

/// Only an `AppImage` updates itself, in place, and only when `can_replace(appimage)` says the
/// user may rename and remove it.
fn resolve(env: &AppImageEnv, can_replace: &dyn Fn(&Path) -> bool) -> Result<PathBuf, String> {
    let Some(RunningAppImage { file: appimage, .. }) = env.running() else {
        return Err("Atrium is not running as an AppImage".into());
    };
    if !appimage.is_absolute() || !appimage.is_file() {
        return Err(format!(
            "the AppImage cannot be found at {}",
            appimage.display()
        ));
    }
    if !can_replace(&appimage) {
        return Err("the folder of the AppImage is not writable for you".into());
    }
    Ok(appimage)
}

/// The sentence the About tab shows when updates have to be installed by hand.
pub(crate) fn manual_only_text(reason: &str) -> String {
    format!("Atrium installs updates itself only when it runs as an AppImage, but {reason}.")
}

/// Whether `/proc/<pid>/stat` exists and the process is no zombie (`Z`) or dead (`X`).
pub(crate) fn process_alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|stat| !matches!(process_state(&stat), Some('Z' | 'X')))
}

/// The state field of `/proc/<pid>/stat`, which follows the parenthesised command name (that
/// may itself contain parentheses and spaces).
fn process_state(stat: &str) -> Option<char> {
    let (_, after_name) = stat.rsplit_once(')')?;
    after_name.trim_start().chars().next()
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;
    pub(crate) use crate::native::single_file::fakes::{fake_asset, fake_staged, verify_staged};
    use crate::staging::EXECUTABLE_MODE;
    use crate::swap::{new_update_path, partial_update_path};
    use crate::version::Version;

    /// The release asset the flow tests use.
    pub(crate) const ASSET: &str = "atrium-linux-x86_64.AppImage";

    /// An executable `Atrium.AppImage` in version 0.1.0 in `<dir>/Applications`; returns its
    /// path.
    pub(crate) fn fake_install(dir: &Path) -> PathBuf {
        let applications = dir.join("Applications");
        std::fs::create_dir_all(&applications).unwrap();
        let install = applications.join("Atrium.AppImage");
        std::fs::write(&install, fake_asset(dir, "0.1.0")).unwrap();
        std::fs::set_permissions(&install, Permissions::from_mode(EXECUTABLE_MODE)).unwrap();
        install
    }

    #[test]
    fn picks_the_asset_of_the_architecture() {
        assert_eq!(
            asset_name_for_arch("x86_64"),
            Some("atrium-linux-x86_64.AppImage")
        );
        assert_eq!(
            asset_name_for_arch("aarch64"),
            Some("atrium-linux-aarch64.AppImage")
        );
        assert_eq!(asset_name_for_arch("riscv64"), None);
    }

    fn env(appimage: Option<&Path>, appdir: Option<&str>) -> AppImageEnv {
        AppImageEnv {
            executable: "/tmp/.mount_atriumAbC/usr/bin/atrium".into(),
            appimage: appimage.map(Path::to_path_buf),
            appdir: appdir.map(PathBuf::from),
        }
    }

    #[test]
    fn an_appimage_in_a_writable_folder_updates_itself() {
        let dir = tempfile::tempdir().unwrap();
        let install = fake_install(dir.path());
        let env = env(Some(&install), Some("/tmp/.mount_atriumAbC"));
        assert_eq!(resolve(&env, &|_| true), Ok(install));
    }

    #[test]
    fn everything_else_only_notifies() {
        let dir = tempfile::tempdir().unwrap();
        let install = fake_install(dir.path());
        let mount = Some("/tmp/.mount_atriumAbC");
        let missing = dir.path().join("Applications/Gone.AppImage");
        let relative = Path::new("Atrium.AppImage");
        let cases = [
            (env(None, mount), true, "not running as an AppImage"),
            (
                env(Some(&install), None),
                true,
                "not running as an AppImage",
            ),
            (
                env(Some(&install), Some("/tmp/.mount_other")),
                true,
                "not running as an AppImage",
            ),
            (env(Some(&missing), mount), true, "cannot be found"),
            (env(Some(relative), mount), true, "cannot be found"),
            (env(Some(&install), mount), false, "not writable"),
        ];
        for (env, replaceable, expected) in cases {
            let reason = resolve(&env, &|_| replaceable).unwrap_err();
            assert!(reason.contains(expected), "{:?}: {reason}", env.appimage);
        }
    }

    #[test]
    fn stages_an_executable_copy_and_replaces_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let install = fake_install(dir.path());
        let downloaded = dir.path().join("download.AppImage");
        std::fs::write(&downloaded, fake_asset(dir.path(), "0.2.0")).unwrap();
        std::fs::set_permissions(&downloaded, Permissions::from_mode(0o600)).unwrap();
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
    fn reads_the_state_after_the_command_name() {
        assert_eq!(process_state("42 (atrium) S 1 42"), Some('S'));
        assert_eq!(process_state("42 (a) b (c)) Z 1 42"), Some('Z'));
        assert_eq!(process_state("garbage"), None);
    }

    #[test]
    fn own_process_is_alive_and_a_missing_one_is_not() {
        assert!(process_alive(std::process::id()));
        assert!(!process_alive(u32::MAX));
    }
}
