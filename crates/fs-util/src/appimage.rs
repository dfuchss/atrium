//! The `AppImage` Atrium runs from on Linux: the updater replaces it, the desktop integration
//! installs its entry.

use std::io;
use std::path::PathBuf;

/// The `AppImage` Atrium runs from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningAppImage {
    /// The `AppImage` file that was started (`$APPIMAGE`).
    pub file: PathBuf,
    /// Where the runtime mounted or extracted it (`$APPDIR`).
    pub dir: PathBuf,
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

    /// The `AppImage` this process runs from, if it does. The executable has to lie below
    /// `$APPDIR`, so an `APPIMAGE` variable inherited by another program does not count.
    pub fn running(&self) -> Option<RunningAppImage> {
        let (file, dir) = (self.appimage.as_ref()?, self.appdir.as_ref()?);
        self.executable.starts_with(dir).then(|| RunningAppImage {
            file: file.clone(),
            dir: dir.clone(),
        })
    }
}

/// The `AppImage` Atrium runs from, or `None` (a source build, an app bundle, or the file is
/// gone).
pub fn running_appimage() -> Option<RunningAppImage> {
    AppImageEnv::from_env()
        .ok()?
        .running()
        .filter(|running| running.file.is_absolute() && running.file.is_file())
}
