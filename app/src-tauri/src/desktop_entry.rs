//! The desktop entry and icons that Atrium installs for its `AppImage` or its release binary on
//! Linux, as plain file operations below the XDG data folder:
//! `applications/org.fuchss.atrium.desktop` and `icons/hicolor/<size>/apps/org.fuchss.atrium.png`.
//! Both are named after Atrium's app id, so the desktop matches the window to the entry, install
//! writes every icon size it is given (those of the `AppImage` or the ones built into the binary)
//! and uninstall removes them from every size folder.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

use fs_util::appimage::RunningAppImage;
use fs_util::write_atomically;
use protocol::DesktopEntryState;

/// The entry of Atrium; `{{exec}}` stands for the quoted path of the program and
/// `{{icon}}` for the name of the icon.
const TEMPLATE: &str = include_str!("../linux/atrium.desktop");
const EXEC_PLACEHOLDER: &str = "{{exec}}";
const ICON_PLACEHOLDER: &str = "{{icon}}";
/// Named after the window's app id (see `host_tauri::set_app_id`).
const ENTRY_FILE: &str = "org.fuchss.atrium.desktop";
/// The name of the installed icons: Atrium's app id.
const ICON_NAME: &str = "org.fuchss.atrium";
/// The icons inside the `AppImage`, below its folder, named after the program.
const BUNDLED_ICONS: &str = "usr/share/icons/hicolor";
const BUNDLED_ICON_FILE: &str = "atrium.png";
/// The icons built into the release binary, as (size folder, PNG); macOS has no desktop entry.
#[cfg(target_os = "linux")]
const EMBEDDED_ICONS: [(&str, &[u8]); 4] = [
    ("32x32", include_bytes!("../icons/32x32.png")),
    ("64x64", include_bytes!("../icons/64x64.png")),
    ("128x128", include_bytes!("../icons/128x128.png")),
    ("256x256", include_bytes!("../icons/128x128@2x.png")),
];
#[cfg(not(target_os = "linux"))]
const EMBEDDED_ICONS: [(&str, &[u8]); 0] = [];
const TRY_EXEC_KEY: &str = "TryExec=";
const FILE_MODE: u32 = 0o644;

/// The folders the entry and the icons go to.
pub(crate) struct DesktopDirs {
    data_home: PathBuf,
}

impl DesktopDirs {
    /// `$XDG_DATA_HOME` if it is an absolute path, else `~/.local/share`.
    pub(crate) fn from_env() -> Result<DesktopDirs, String> {
        let from_var = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        from_var("XDG_DATA_HOME")
            .or_else(|| from_var("HOME").map(|home| home.join(".local/share")))
            .map(|data_home| DesktopDirs { data_home })
            .ok_or_else(|| "cannot find your data folder (XDG_DATA_HOME, HOME)".to_string())
    }

    fn entry_path(&self) -> PathBuf {
        self.data_home.join("applications").join(ENTRY_FILE)
    }

    fn icon_root(&self) -> PathBuf {
        self.data_home.join("icons/hicolor")
    }

    fn icon_path(&self, size: &OsStr) -> PathBuf {
        self.icon_root().join(size).join("apps").join(icon_file())
    }
}

fn icon_file() -> String {
    format!("{ICON_NAME}.png")
}

/// Quotes `program` for `Exec` as the Desktop Entry specification wants: in double quotes
/// with `"`, `` ` ``, `$` and `\` escaped, `%` doubled, and every backslash doubled again for
/// the string value of the key.
fn quote_program(program: &str) -> String {
    let mut quoted = String::from("\"");
    for character in program.chars() {
        match character {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted.replace('\\', "\\\\")
}

/// A value of the entry's string keys: backslashes doubled.
fn escape_value(value: &str) -> String {
    value.replace('\\', "\\\\")
}

/// The value written by [`escape_value`].
fn unescape_value(value: &str) -> String {
    value.replace("\\\\", "\\")
}

/// The entry that starts `file`.
fn render(file: &Path) -> Result<String, String> {
    let program = file
        .to_str()
        .filter(|program| !program.chars().any(char::is_control))
        .ok_or_else(|| format!("the path of Atrium cannot be used: {}", file.display()))?;
    let mut entry = TEMPLATE
        .replace(EXEC_PLACEHOLDER, &quote_program(program))
        .replace(ICON_PLACEHOLDER, ICON_NAME);
    if !entry.ends_with('\n') {
        entry.push('\n');
    }
    entry.push_str(TRY_EXEC_KEY);
    entry.push_str(&escape_value(program));
    entry.push('\n');
    Ok(entry)
}

/// Whether the entry exists and starts `file`, as its `TryExec` says.
pub(crate) fn state(dirs: &DesktopDirs, file: &Path) -> Result<DesktopEntryState, String> {
    let text = match std::fs::read_to_string(dirs.entry_path()) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(DesktopEntryState::NotInstalled);
        }
        Err(error) => return Err(format!("cannot read the entry: {error}")),
    };
    let program = text
        .lines()
        .find_map(|line| line.strip_prefix(TRY_EXEC_KEY))
        .map(unescape_value);
    Ok(match program {
        Some(program) if Path::new(&program) == file => DesktopEntryState::Installed,
        program => DesktopEntryState::OtherPath {
            path: program.unwrap_or_else(|| "unknown".into()),
        },
    })
}

/// The icons named `file` in the size folders `<size>/apps` below `root`, as (size, file); none
/// if `root` does not exist.
fn icons_in(root: &Path, file: &str) -> io::Result<Vec<(OsString, PathBuf)>> {
    let folders = match std::fs::read_dir(root) {
        Ok(folders) => folders,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut icons = Vec::new();
    for folder in folders {
        let folder = folder?;
        let icon = folder.path().join("apps").join(file);
        if icon.is_file() {
            icons.push((folder.file_name(), icon));
        }
    }
    Ok(icons)
}

/// The icons of every size the running `AppImage` has, as (size folder, PNG).
pub(crate) fn appimage_icons(
    running: &RunningAppImage,
) -> Result<Vec<(OsString, Vec<u8>)>, String> {
    let bundled = icons_in(&running.dir.join(BUNDLED_ICONS), BUNDLED_ICON_FILE)
        .map_err(|error| format!("cannot read the icons of the AppImage: {error}"))?;
    if bundled.is_empty() {
        return Err("the AppImage contains no icon".into());
    }
    bundled
        .into_iter()
        .map(|(size, icon)| match std::fs::read(&icon) {
            Ok(bytes) => Ok((size, bytes)),
            Err(error) => Err(format!("cannot read {}: {error}", icon.display())),
        })
        .collect()
}

/// The icons built into the release binary, as (size folder, PNG).
pub(crate) fn embedded_icons() -> Vec<(OsString, Vec<u8>)> {
    EMBEDDED_ICONS
        .iter()
        .map(|(size, png)| (OsString::from(size), png.to_vec()))
        .collect()
}

/// Installs or rewrites the entry that starts `file` and the `icons` (size folder, PNG).
pub(crate) fn install(
    dirs: &DesktopDirs,
    file: &Path,
    icons: &[(OsString, Vec<u8>)],
) -> Result<(), String> {
    for (size, png) in icons {
        write_file(&dirs.icon_path(size), png)?;
    }
    write_file(&dirs.entry_path(), render(file)?.as_bytes())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_atomically(path, bytes, FILE_MODE)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// Removes the file at `path`, if there is one.
fn remove_file(path: &Path) -> Result<(), String> {
    fs_util::remove_if_present(path)
        .map_err(|error| format!("cannot remove {}: {error}", path.display()))
}

/// Removes Atrium's entry and its icons of every size.
pub(crate) fn uninstall(dirs: &DesktopDirs) -> Result<(), String> {
    let icon_root = dirs.icon_root();
    let icons = icons_in(&icon_root, &icon_file())
        .map_err(|error| format!("cannot read {}: {error}", icon_root.display()))?;
    for (_, icon) in icons {
        remove_file(&icon)?;
    }
    remove_file(&dirs.entry_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs_in(root: &Path) -> DesktopDirs {
        DesktopDirs {
            data_home: root.join("data"),
        }
    }

    fn size(name: &str) -> OsString {
        OsString::from(name)
    }

    /// A fake mounted `AppImage` with icons of two sizes, and the file it was started from.
    fn running_in(root: &Path, file_name: &str) -> RunningAppImage {
        running_with_icons(root, file_name, &["32x32", "128x128"])
    }

    /// A fake mounted `AppImage` with icons of the `sizes`, and the file it was started from.
    fn running_with_icons(root: &Path, file_name: &str, sizes: &[&str]) -> RunningAppImage {
        let dir = root.join("mount");
        for size in sizes {
            let folder = dir.join(BUNDLED_ICONS).join(size).join("apps");
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(BUNDLED_ICON_FILE), size).unwrap();
        }
        let file = root.join(file_name);
        std::fs::write(&file, "appimage").unwrap();
        RunningAppImage { file, dir }
    }

    /// Installs the entry and the icons of the `AppImage`, as Settings does.
    fn install_appimage(dirs: &DesktopDirs, running: &RunningAppImage) -> Result<(), String> {
        install(dirs, &running.file, &appimage_icons(running)?)
    }

    #[test]
    fn entry_quotes_the_program() {
        let entry = render(Path::new("/home/u/My Apps/Atrium.AppImage")).unwrap();
        assert!(entry.contains("\nExec=\"/home/u/My Apps/Atrium.AppImage\" %U\n"));
        for line in [
            "Type=Application",
            "Name=Atrium",
            "Icon=org.fuchss.atrium",
            "StartupWMClass=org.fuchss.atrium",
            "TryExec=/home/u/My Apps/Atrium.AppImage",
        ] {
            assert!(entry.lines().any(|found| found == line), "{line}");
        }
        assert!(!entry.contains("{{"));
        assert!(entry.ends_with('\n'));
    }

    #[test]
    fn quoting_survives_special_characters() {
        let program = "/a b/\"q\"/`t`/$v/back\\slash/50%/Atrium";
        let entry = render(Path::new(program)).unwrap();
        let exec = entry
            .lines()
            .find_map(|line| line.strip_prefix("Exec="))
            .unwrap();
        assert_eq!(
            exec,
            r#""/a b/\\"q\\"/\\`t\\`/\\$v/back\\\\slash/50%%/Atrium" %U"#
        );
        assert!(render(Path::new("/a\nb")).is_err());
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_in(root.path());
        write_file(&dirs.entry_path(), entry.as_bytes()).unwrap();
        assert_eq!(
            state(&dirs, Path::new(program)).unwrap(),
            DesktopEntryState::Installed
        );
    }

    #[test]
    fn state_follows_the_installed_entry() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_in(root.path());
        let running = running_in(root.path(), "Atrium.AppImage");
        assert_eq!(
            state(&dirs, &running.file).unwrap(),
            DesktopEntryState::NotInstalled
        );
        install_appimage(&dirs, &running).unwrap();
        assert_eq!(
            state(&dirs, &running.file).unwrap(),
            DesktopEntryState::Installed
        );
        let moved = root.path().join("Moved.AppImage");
        assert_eq!(
            state(&dirs, &moved).unwrap(),
            DesktopEntryState::OtherPath {
                path: running.file.display().to_string()
            }
        );
    }

    #[test]
    fn install_writes_entry_and_icons_and_updates() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_in(root.path());
        let running = running_in(root.path(), "Atrium.AppImage");
        install_appimage(&dirs, &running).unwrap();
        for name in ["32x32", "128x128"] {
            let icon = dirs.icon_path(&size(name));
            assert_eq!(std::fs::read_to_string(icon).unwrap(), name);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let mode = std::fs::metadata(dirs.entry_path()).unwrap().permissions();
            assert_eq!(mode.mode() & 0o777, FILE_MODE);
        }
        let leftovers = std::fs::read_dir(dirs.entry_path().parent().unwrap())
            .unwrap()
            .count();
        assert_eq!(leftovers, 1);

        let moved = running_in(root.path(), "Renamed.AppImage");
        install_appimage(&dirs, &moved).unwrap();
        assert_eq!(
            state(&dirs, &moved.file).unwrap(),
            DesktopEntryState::Installed
        );
    }

    #[test]
    fn install_overwrites_the_icons() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_in(root.path());
        let running = running_in(root.path(), "Atrium.AppImage");
        let icon = dirs.icon_path(&size("32x32"));
        std::fs::create_dir_all(icon.parent().unwrap()).unwrap();
        std::fs::write(&icon, "old").unwrap();
        install_appimage(&dirs, &running).unwrap();
        assert_eq!(std::fs::read_to_string(icon).unwrap(), "32x32");
    }

    #[test]
    fn uninstall_removes_the_entry_and_the_icon_of_every_size() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_in(root.path());
        install_appimage(&dirs, &running_in(root.path(), "Atrium.AppImage")).unwrap();
        let older = dirs.icon_path(&size("256x256@2"));
        std::fs::create_dir_all(older.parent().unwrap()).unwrap();
        std::fs::write(&older, "older").unwrap();
        let other = dirs.icon_root().join("32x32/apps/atrium.png");
        std::fs::write(&other, "theirs").unwrap();

        uninstall(&dirs).unwrap();
        assert!(!dirs.entry_path().exists());
        for name in ["32x32", "128x128", "256x256@2"] {
            assert!(!dirs.icon_path(&size(name)).exists(), "{name}");
        }
        assert!(other.exists());
        uninstall(&dirs).unwrap();
    }

    #[test]
    fn icons_are_found_in_size_folders_only() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            icons_in(&root.path().join("missing"), "a.png")
                .unwrap()
                .is_empty()
        );
        for folder in ["32x32/apps", "64x64/apps", "64x64/other"] {
            std::fs::create_dir_all(root.path().join(folder)).unwrap();
        }
        std::fs::write(root.path().join("32x32/apps/a.png"), "a").unwrap();
        std::fs::write(root.path().join("64x64/other/a.png"), "a").unwrap();
        std::fs::write(root.path().join("a.png"), "a").unwrap();
        assert_eq!(
            icons_in(root.path(), "a.png").unwrap(),
            [(size("32x32"), root.path().join("32x32/apps/a.png"))]
        );
    }

    #[test]
    fn install_needs_an_icon() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_in(root.path());
        let running = running_with_icons(root.path(), "Atrium.AppImage", &[]);
        assert!(install_appimage(&dirs, &running).is_err());
        assert!(!dirs.entry_path().exists());
    }

    #[test]
    fn embedded_icons_have_the_size_of_their_folder() {
        for (size, png) in embedded_icons() {
            let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
            assert_eq!(size, OsString::from(format!("{width}x{height}")));
        }
    }

    #[test]
    fn install_writes_the_given_icons_for_a_binary() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_in(root.path());
        let binary = root.path().join("bin/atrium");
        let icons = [(size("48x48"), b"png".to_vec())];
        install(&dirs, &binary, &icons).unwrap();
        assert_eq!(
            std::fs::read(dirs.icon_path(&size("48x48"))).unwrap(),
            b"png"
        );
        assert_eq!(state(&dirs, &binary).unwrap(), DesktopEntryState::Installed);
    }
}
