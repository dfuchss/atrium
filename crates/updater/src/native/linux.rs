//! Linux: the release is an `AppImage` that replaces the running one in place (in any folder the
//! user may write to, the file name is kept); the user opens Atrium again after the swap.

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use crate::location::{InstallLocation, RunningAppImage, can_replace};
use crate::staging::{EXECUTABLE_MODE, place_next_to_install};
use crate::version::Version;

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

/// What the `AppImage` runtime tells the running app about itself.
struct AppImageEnv {
    /// The running executable, inside the mounted (or extracted) `AppImage`.
    executable: PathBuf,
    /// `$APPIMAGE`: the `AppImage` file that was started.
    appimage: Option<PathBuf>,
    /// `$APPDIR` without symlinks (like `executable`): where the runtime mounted or extracted
    /// it.
    appdir: Option<PathBuf>,
}

impl AppImageEnv {
    fn from_env() -> std::io::Result<AppImageEnv> {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Ok(AppImageEnv {
            executable: std::env::current_exe()?,
            appimage: var("APPIMAGE"),
            appdir: var("APPDIR").and_then(|dir| std::fs::canonicalize(dir).ok()),
        })
    }
}

/// Where the running executable is installed, and whether it may replace itself.
pub fn install_location(debug_build: bool) -> InstallLocation {
    match AppImageEnv::from_env() {
        Ok(env) => resolve(&env, debug_build, &can_replace),
        Err(error) => InstallLocation::ManualOnly {
            reason: format!("cannot locate Atrium: {error}"),
        },
    }
}

/// The `AppImage` this process runs from, if it does. The executable has to lie below
/// `$APPDIR`, so an `APPIMAGE` variable inherited by another program does not count.
fn running_from(env: &AppImageEnv) -> Option<RunningAppImage> {
    let (file, dir) = (env.appimage.as_ref()?, env.appdir.as_ref()?);
    env.executable.starts_with(dir).then(|| RunningAppImage {
        file: file.clone(),
        dir: dir.clone(),
    })
}

/// The `AppImage` Atrium runs from, or `None` (a source build, or the file is gone).
pub fn running_appimage() -> Option<RunningAppImage> {
    running_from(&AppImageEnv::from_env().ok()?)
        .filter(|running| running.file.is_absolute() && running.file.is_file())
}

/// Only an `AppImage` updates itself, in place, and only when `can_replace(appimage)` says the
/// user may rename and remove it.
fn resolve(
    env: &AppImageEnv,
    debug_build: bool,
    can_replace: &dyn Fn(&Path) -> bool,
) -> InstallLocation {
    let manual = |reason: String| InstallLocation::ManualOnly { reason };
    if debug_build {
        return manual("this is a development build".into());
    }
    let Some(RunningAppImage { file: appimage, .. }) = running_from(env) else {
        return manual("Atrium is not running as an AppImage".into());
    };
    if !appimage.is_absolute() || !appimage.is_file() {
        return manual(format!(
            "the AppImage cannot be found at {}",
            appimage.display()
        ));
    }
    if !can_replace(&appimage) {
        return manual("the folder of the AppImage is not writable for you".into());
    }
    InstallLocation::SelfUpdatable { bundle: appimage }
}

/// Why a release without the file for this machine is only announced.
pub(crate) const NO_DOWNLOAD_REASON: &str = "the release has no download for this computer";

/// The sentence the About tab shows when updates have to be installed by hand.
pub(crate) fn manual_only_text(reason: &str) -> String {
    format!("Atrium installs updates itself only when it runs as an AppImage, but {reason}.")
}

/// Moves the verified download next to the install (`<install>.new-update`, executable), which
/// it returns. Identity and version come from `SHA256SUMS` and the tag.
pub(crate) fn prepare_update(
    downloaded: &Path,
    _version: Version,
    install: &Path,
) -> Result<PathBuf, String> {
    std::fs::set_permissions(downloaded, Permissions::from_mode(EXECUTABLE_MODE))
        .map_err(|error| format!("cannot make the update executable: {error}"))?;
    place_next_to_install(downloaded, install)
}

/// The helper is started through the `AppImage` (`install`): the running executable lives in
/// the `AppImage`'s mount, which goes away when the app exits.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the platforms share this signature; macOS can fail"
)]
pub(crate) fn helper_executable(install: &Path) -> std::io::Result<PathBuf> {
    Ok(install.to_path_buf())
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

/// Always empty: renaming the `AppImage` while another instance runs from it is safe on Linux;
/// that instance keeps the old version until it restarts.
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
    pub(crate) const ASSET: &str = "atrium-linux-x86_64.AppImage";

    /// The release asset of Atrium in `version`: a file holding the version.
    pub(crate) fn fake_asset(_dir: &Path, version: &str) -> Vec<u8> {
        version.as_bytes().to_vec()
    }

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

    /// Stages the `AppImage` of `version` next to `install`.
    pub(crate) fn fake_staged(dir: &Path, install: &Path, version: &str) {
        let staged = new_update_path(install);
        std::fs::write(&staged, fake_asset(dir, version)).unwrap();
        std::fs::set_permissions(&staged, Permissions::from_mode(EXECUTABLE_MODE)).unwrap();
    }

    /// Fails unless `path` is the executable `AppImage` of `version`.
    pub(crate) fn verify_staged(path: &Path, version: &str) {
        let mode = std::fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, EXECUTABLE_MODE, "{}", path.display());
        assert_eq!(std::fs::read(path).unwrap(), fake_asset(path, version));
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
        assert_eq!(
            resolve(&env, false, &|_| true),
            InstallLocation::SelfUpdatable { bundle: install }
        );
    }

    #[test]
    fn everything_else_only_notifies() {
        let dir = tempfile::tempdir().unwrap();
        let install = fake_install(dir.path());
        let mount = Some("/tmp/.mount_atriumAbC");
        let missing = dir.path().join("Applications/Gone.AppImage");
        let relative = Path::new("Atrium.AppImage");
        let cases = [
            (env(Some(&install), mount), true, true, "development"),
            (env(None, mount), false, true, "not running as an AppImage"),
            (
                env(Some(&install), None),
                false,
                true,
                "not running as an AppImage",
            ),
            (
                env(Some(&install), Some("/tmp/.mount_other")),
                false,
                true,
                "not running as an AppImage",
            ),
            (env(Some(&missing), mount), false, true, "cannot be found"),
            (env(Some(relative), mount), false, true, "cannot be found"),
            (env(Some(&install), mount), false, false, "not writable"),
        ];
        for (env, debug, replaceable, expected) in cases {
            let location = resolve(&env, debug, &|_| replaceable);
            assert!(
                location
                    .manual_reason()
                    .unwrap_or_default()
                    .contains(expected),
                "{:?}: {location:?}",
                env.appimage
            );
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
