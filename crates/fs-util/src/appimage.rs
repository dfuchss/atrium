//! How Atrium is packaged on Linux, the `AppImage` or the plain release binary: the updater
//! replaces that file, the desktop integration installs its entry.

use std::io;
use std::path::{Path, PathBuf};

/// The `AppImage` Atrium runs from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningAppImage {
    /// The `AppImage` file that was started (`$APPIMAGE`).
    pub file: PathBuf,
    /// Where the runtime mounted or extracted it (`$APPDIR`).
    pub dir: PathBuf,
}

/// How the running Atrium is packaged on Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packaging {
    /// Started from the `AppImage`.
    AppImage(RunningAppImage),
    /// Started as the program file itself (the release binary or a build from source).
    Binary(PathBuf),
}

impl Packaging {
    /// The file the user started: the `AppImage` or the program.
    pub fn file(&self) -> &Path {
        match self {
            Packaging::AppImage(running) => &running.file,
            Packaging::Binary(executable) => executable,
        }
    }
}

/// What the `AppImage` runtime tells the running app about itself.
pub struct AppImageEnv {
    /// The running executable, inside the mounted (or extracted) `AppImage`.
    pub executable: PathBuf,
    /// `$APPIMAGE`: the `AppImage` file that was started.
    pub appimage: Option<PathBuf>,
    /// `$APPDIR` without symlinks (like `executable`): where the runtime mounted or extracted
    /// it.
    pub appdir: Option<PathBuf>,
}

impl AppImageEnv {
    pub fn from_env() -> io::Result<AppImageEnv> {
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

    /// The `AppImage` this process runs from, else the running executable. The executable has
    /// to lie below `$APPDIR`, so an `APPIMAGE` variable inherited by another program does not
    /// count.
    pub fn packaging(&self) -> Packaging {
        match (&self.appimage, &self.appdir) {
            (Some(file), Some(dir)) if self.executable.starts_with(dir) => {
                Packaging::AppImage(RunningAppImage {
                    file: file.clone(),
                    dir: dir.clone(),
                })
            }
            _ => Packaging::Binary(self.executable.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(appimage: Option<&str>, appdir: Option<&str>) -> AppImageEnv {
        AppImageEnv {
            executable: "/tmp/.mount_atriumAbC/usr/bin/atrium".into(),
            appimage: appimage.map(PathBuf::from),
            appdir: appdir.map(PathBuf::from),
        }
    }

    #[test]
    fn only_an_executable_in_the_mount_runs_as_the_appimage() {
        let mount = Some("/tmp/.mount_atriumAbC");
        let appimage = Some("/home/u/Atrium.AppImage");
        assert_eq!(
            env(appimage, mount).packaging(),
            Packaging::AppImage(RunningAppImage {
                file: "/home/u/Atrium.AppImage".into(),
                dir: "/tmp/.mount_atriumAbC".into(),
            })
        );
        let binary = Packaging::Binary("/tmp/.mount_atriumAbC/usr/bin/atrium".into());
        for other in [
            env(None, mount),
            env(appimage, None),
            env(appimage, Some("/tmp/.mount_other")),
        ] {
            assert_eq!(other.packaging(), binary);
        }
    }
}
