//! macOS: the release is a zip of `Atrium.app`, only bundles in an Applications folder update
//! themselves, and the helper opens the swapped bundle again.

mod archive;
mod bundle;
mod quarantine;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::location::{self, InstallLocation};
use crate::staging::place_next_to_install;
use crate::version::Version;

/// The helper opens the swapped bundle again.
pub(crate) const RELAUNCHES_AFTER_SWAP: bool = true;
/// The bundle is a folder, which a rename cannot replace: it is moved aside first.
pub(crate) const SWAPS_IN_ONE_RENAME: bool = false;

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
    location::locate(
        debug_build,
        std::env::current_exe(),
        |executable, can_replace| resolve(executable, home.as_deref(), can_replace),
    )
}

/// Decides for the running `executable` (`<bundle>/Contents/MacOS/<name>`). Only bundles
/// directly in `/Applications` or `~/Applications` update themselves, and only when
/// `can_replace(bundle)` says the user may rename and remove it.
fn resolve(
    executable: &Path,
    home: Option<&Path>,
    can_replace: &dyn Fn(&Path) -> bool,
) -> Result<PathBuf, String> {
    let Some(bundle) = bundle_of(executable) else {
        return Err("Atrium is not running from an app bundle".into());
    };
    if bundle.to_string_lossy().contains("/AppTranslocation/") {
        return Err("macOS runs Atrium from a temporary location; move it to Applications".into());
    }
    let in_applications = bundle.parent().is_some_and(|parent| {
        parent == Path::new("/Applications")
            || home.is_some_and(|home| parent == home.join("Applications"))
    });
    if !in_applications {
        return Err("Atrium is not installed in the Applications folder".into());
    }
    if !can_replace(&bundle) {
        return Err("the Applications folder or Atrium.app is not writable for you".into());
    }
    Ok(bundle)
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
    let staged = place_next_to_install(&bundle, install, copy_bundle)?;
    let _ = std::fs::remove_dir_all(&extracted);
    Ok(staged)
}

/// Copies the bundle with `ditto` (which may set the quarantine again, so it is cleared).
fn copy_bundle(bundle: &Path, to: &Path) -> std::io::Result<()> {
    let status = Command::new("/usr/bin/ditto")
        .arg(bundle)
        .arg(to)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!("ditto failed: {status}")));
    }
    quarantine::remove_quarantine(to).map_err(std::io::Error::other)
}

/// The helper is the app's own (installed, already trusted) executable.
pub(crate) fn helper_executable(_install: &Path) -> std::io::Result<PathBuf> {
    std::env::current_exe()
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
pub(crate) mod tests {
    use super::*;
    pub(crate) use crate::native::macos::bundle::BUNDLE_IDENTIFIER;
    use crate::native::macos::bundle::tests::fake_bundle;
    use crate::swap::new_update_path;

    /// The release asset the flow tests use.
    pub(crate) const ASSET: &str = "atrium-macos-arm64.zip";

    /// An `Atrium.app` in version 0.1.0 in `<dir>/Applications`; returns its path.
    pub(crate) fn fake_install(dir: &Path) -> PathBuf {
        let applications = dir.join("Applications");
        std::fs::create_dir_all(&applications).unwrap();
        fake_bundle(&applications, BUNDLE_IDENTIFIER, "0.1.0")
    }

    /// The release asset of Atrium in `version`, built in a folder below `dir`.
    pub(crate) fn fake_asset(dir: &Path, version: &str) -> Vec<u8> {
        fake_zip(dir, BUNDLE_IDENTIFIER, version)
    }

    /// The zip of an `Atrium.app` with the given identifier and version.
    pub(crate) fn fake_zip(dir: &Path, identifier: &str, version: &str) -> Vec<u8> {
        let build = dir.join(format!("build-{version}"));
        let bundle = fake_bundle(&build, identifier, version);
        let zip = build.join(ASSET);
        let status = Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&bundle)
            .arg(&zip)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::read(zip).unwrap()
    }

    /// Stages an `Atrium.app` in `version`, built in a folder below `dir`, next to `install`.
    pub(crate) fn fake_staged(dir: &Path, install: &Path, version: &str) {
        let build = dir.join(format!("staged-{version}"));
        let bundle = fake_bundle(&build, BUNDLE_IDENTIFIER, version);
        std::fs::rename(bundle, new_update_path(install)).unwrap();
    }

    /// Fails unless `path` is a complete Atrium in `version`.
    pub(crate) fn verify_staged(path: &Path, version: &str) {
        bundle::verify(path, Version::parse(version).unwrap()).unwrap();
    }

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

    fn resolve_for(executable: &str, replaceable: bool) -> Result<PathBuf, String> {
        resolve(
            Path::new(executable),
            Some(Path::new("/Users/test")),
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
            assert_eq!(resolve_for(executable, true), Ok(bundle.into()));
        }
    }

    #[test]
    fn everything_else_only_notifies() {
        let cases = [
            (
                "/Users/test/src/atrium/target/release/atrium",
                true,
                "app bundle",
            ),
            (
                "/private/var/folders/x/AppTranslocation/1/d/Atrium.app/Contents/MacOS/atrium",
                true,
                "temporary",
            ),
            (
                "/Users/test/Downloads/Atrium.app/Contents/MacOS/atrium",
                true,
                "Applications",
            ),
            (
                "/Volumes/Atrium/Atrium.app/Contents/MacOS/atrium",
                true,
                "Applications",
            ),
            (
                "/Applications/Tools/Atrium.app/Contents/MacOS/atrium",
                true,
                "Applications",
            ),
            (
                "/Applications/Atrium.app/Contents/MacOS/atrium",
                false,
                "writable",
            ),
        ];
        for (executable, replaceable, expected) in cases {
            let reason = resolve_for(executable, replaceable).unwrap_err();
            assert!(reason.contains(expected), "{executable}: {reason}");
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
