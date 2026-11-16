//! macOS: the release is a zip of `Atrium.app`, only bundles in an Applications folder update
//! themselves, and the helper opens the swapped bundle again.

mod archive;
pub(crate) mod bundle;
mod quarantine;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::location::{InstallLocation, can_replace};
use crate::staging::remove_if_present;
use crate::swap::{new_update_path, partial_update_path};
use crate::version::Version;

/// The release asset for a Mac of the given architecture (`std::env::consts::ARCH`); the
/// names are fixed by `.github/workflows/release.yml`.
pub fn asset_name_for_arch(arch: &str) -> Option<&'static str> {
    match arch {
        "aarch64" => Some("atrium-macos-arm64.zip"),
        "x86_64" => Some("atrium-macos-x86_64.zip"),
        _ => None,
    }
}

/// Where the running executable is installed, and whether it may replace itself.
pub fn install_location(debug_build: bool) -> InstallLocation {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match std::env::current_exe() {
        Ok(executable) => resolve(&executable, home.as_deref(), debug_build, &can_replace),
        Err(error) => InstallLocation::ManualOnly {
            reason: format!("cannot locate Atrium: {error}"),
        },
    }
}

/// Decides for the running `executable` (`<bundle>/Contents/MacOS/<name>`). Only bundles
/// directly in `/Applications` or `~/Applications` update themselves, and only when
/// `can_replace(bundle)` says the user may rename and remove it.
fn resolve(
    executable: &Path,
    home: Option<&Path>,
    debug_build: bool,
    can_replace: &dyn Fn(&Path) -> bool,
) -> InstallLocation {
    let manual = |reason: &str| InstallLocation::ManualOnly {
        reason: reason.to_string(),
    };
    if debug_build {
        return manual("this is a development build");
    }
    let Some(bundle) = bundle_of(executable) else {
        return manual("Atrium is not running from an app bundle");
    };
    if bundle.to_string_lossy().contains("/AppTranslocation/") {
        return manual("macOS runs Atrium from a temporary location; move it to Applications");
    }
    let in_applications = bundle.parent().is_some_and(|parent| {
        parent == Path::new("/Applications")
            || home.is_some_and(|home| parent == home.join("Applications"))
    });
    if !in_applications {
        return manual("Atrium is not installed in the Applications folder");
    }
    if !can_replace(&bundle) {
        return manual("the Applications folder or Atrium.app is not writable for you");
    }
    InstallLocation::SelfUpdatable { bundle }
}

/// `<bundle>.app` of `<bundle>.app/Contents/MacOS/<executable>`.
fn bundle_of(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    is_bundle.then(|| bundle.to_path_buf())
}

/// Why a release without the file for this machine is only announced.
pub(crate) const NO_DOWNLOAD_REASON: &str = "the release has no download for this Mac";

/// The sentence the About tab shows when updates have to be installed by hand.
pub(crate) fn manual_only_text(reason: &str) -> String {
    format!("Atrium installs updates itself only from the Applications folder, but {reason}.")
}

/// Extracts the verified zip `downloaded` next to it, checks the bundle and moves it next to
/// the install (`<install>.new-update`), which it returns.
pub(crate) fn prepare_update(
    downloaded: &Path,
    version: Version,
    install: &Path,
) -> Result<PathBuf, String> {
    let extracted = downloaded.with_file_name("extracted");
    archive::extract(downloaded, &extracted)?;
    let bundle = bundle::find_bundle(&extracted)?;
    bundle::verify(&bundle, version)?;
    quarantine::remove_quarantine(&bundle)?;
    let staged = place_next_to_install(&bundle, install)?;
    let _ = std::fs::remove_dir_all(&extracted);
    Ok(staged)
}

/// Moves the checked bundle to `<install>.new-update`, replacing a leftover. Across volumes
/// it is copied with `ditto` to a `.partial` sibling first, so a broken copy never looks
/// complete; the copy is cleared of the quarantine `ditto` may have set again.
fn place_next_to_install(bundle: &Path, install: &Path) -> Result<PathBuf, String> {
    let target = new_update_path(install);
    let partial = partial_update_path(install);
    remove_if_present(&partial)?;
    remove_if_present(&target)?;
    if std::fs::rename(bundle, &target).is_ok() {
        return Ok(target);
    }
    let copied = Command::new("/usr/bin/ditto")
        .arg(bundle)
        .arg(&partial)
        .status()
        .is_ok_and(|status| status.success());
    if copied
        && quarantine::remove_quarantine(&partial).is_ok()
        && std::fs::rename(&partial, &target).is_ok()
    {
        return Ok(target);
    }
    let _ = remove_if_present(&partial);
    Err("cannot place the update next to the installation".into())
}

/// `ps -p` exits with 0 while the process exists.
pub(crate) fn process_alive(pid: u32) -> bool {
    Command::new("/bin/ps")
        .args(["-p", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Processes other than this one running from `<install>` (a second instance started from
/// the same bundle).
pub(crate) fn processes_running_from(install: &Path) -> Result<Vec<u32>, String> {
    processes_running_below(&install.join("Contents/MacOS"))
}

/// Processes other than this one whose executable lies in `macos_dir`, from `ps`.
fn processes_running_below(macos_dir: &Path) -> Result<Vec<u32>, String> {
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,comm="])
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("cannot run ps: {error}"))?;
    if !output.status.success() {
        return Err(format!("ps failed: {}", output.status));
    }
    Ok(pids_running_from(
        &String::from_utf8_lossy(&output.stdout),
        macos_dir,
        std::process::id(),
    ))
}

/// The ids in `ps -o pid=,comm=` output (`comm` is the executable's full path on macOS) of
/// processes other than `own_pid` running an executable below `macos_dir`.
fn pids_running_from(ps_output: &str, macos_dir: &Path, own_pid: u32) -> Vec<u32> {
    ps_output
        .lines()
        .filter_map(|line| {
            let (pid, executable) = line.trim_start().split_once(' ')?;
            let pid: u32 = pid.parse().ok()?;
            let inside = Path::new(executable.trim_start()).starts_with(macos_dir);
            (inside && pid != own_pid).then_some(pid)
        })
        .collect()
}

/// Opens the installation (`open` only activates an instance that still runs).
pub(crate) fn relaunch(install: &Path) {
    let _ = Command::new("/usr/bin/open").arg(install).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_asset_of_the_architecture() {
        assert_eq!(
            asset_name_for_arch("aarch64"),
            Some("atrium-macos-arm64.zip")
        );
        assert_eq!(
            asset_name_for_arch("x86_64"),
            Some("atrium-macos-x86_64.zip")
        );
        assert_eq!(asset_name_for_arch("riscv64"), None);
    }

    fn resolve_for(executable: &str, debug: bool, replaceable: bool) -> InstallLocation {
        resolve(
            Path::new(executable),
            Some(Path::new("/Users/test")),
            debug,
            &|_| replaceable,
        )
    }

    #[test]
    fn bundles_in_applications_update_themselves() {
        for (executable, bundle) in [
            (
                "/Applications/Atrium.app/Contents/MacOS/atrium",
                "/Applications/Atrium.app",
            ),
            (
                "/Users/test/Applications/Atrium.app/Contents/MacOS/Atrium",
                "/Users/test/Applications/Atrium.app",
            ),
        ] {
            assert_eq!(
                resolve_for(executable, false, true),
                InstallLocation::SelfUpdatable {
                    bundle: bundle.into()
                }
            );
        }
    }

    #[test]
    fn everything_else_only_notifies() {
        let cases = [
            (
                "/Applications/Atrium.app/Contents/MacOS/atrium",
                true,
                true,
                "development",
            ),
            (
                "/Users/test/src/atrium/target/release/atrium",
                false,
                true,
                "app bundle",
            ),
            (
                "/private/var/folders/x/AppTranslocation/1/d/Atrium.app/Contents/MacOS/atrium",
                false,
                true,
                "temporary",
            ),
            (
                "/Users/test/Downloads/Atrium.app/Contents/MacOS/atrium",
                false,
                true,
                "Applications",
            ),
            (
                "/Volumes/Atrium/Atrium.app/Contents/MacOS/atrium",
                false,
                true,
                "Applications",
            ),
            (
                "/Applications/Tools/Atrium.app/Contents/MacOS/atrium",
                false,
                true,
                "Applications",
            ),
            (
                "/Applications/Atrium.app/Contents/MacOS/atrium",
                false,
                false,
                "writable",
            ),
        ];
        for (executable, debug, replaceable, expected) in cases {
            let location = resolve_for(executable, debug, replaceable);
            assert!(
                location
                    .manual_reason()
                    .unwrap_or_default()
                    .contains(expected),
                "{executable}: {location:?}"
            );
        }
    }

    #[test]
    fn finds_other_processes_running_from_the_bundle() {
        let ps_output = "\
    1 /sbin/launchd
  815 /Applications/Atrium.app/Contents/MacOS/atrium
  900 /Applications/Atrium.app/Contents/MacOS/atrium
  901 /Applications/Atrium.app.new-update/Contents/MacOS/atrium
  902 /Applications/Atrium Beta.app/Contents/MacOS/atrium
garbage
";
        let macos_dir = Path::new("/Applications/Atrium.app/Contents/MacOS");
        assert_eq!(pids_running_from(ps_output, macos_dir, 900), vec![815]);
        let beta = Path::new("/Applications/Atrium Beta.app/Contents/MacOS");
        assert_eq!(pids_running_from(ps_output, beta, 900), vec![902]);
    }

    #[test]
    fn the_own_process_is_not_counted() {
        let executable = std::env::current_exe().unwrap();
        let running = processes_running_below(executable.parent().unwrap()).unwrap();
        assert!(!running.contains(&std::process::id()), "{running:?}");
    }
}
