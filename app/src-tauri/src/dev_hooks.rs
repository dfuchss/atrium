//! Development hooks of debug builds.

use std::path::PathBuf;

use tauri::WebviewUrl;

/// Development hooks, read from environment variables in debug builds only ("Debug
/// options" in the wiki). Used to run a second, isolated instance with demo data, e.g. for screenshots.
#[derive(Default)]
pub(crate) struct DevHooks {
    /// `ATRIUM_CONFIG_DIR`: config folder (store, services folder, log) instead of the app's.
    pub(crate) config_dir: Option<PathBuf>,
    /// `ATRIUM_ALWAYS_ON_TOP=1`: keep the main window on top (occluded windows are not painted).
    pub(crate) always_on_top: bool,
    /// `ATRIUM_WINDOW_POSITION=<x>,<y>`: main window position in logical px (global screen
    /// coordinates, the main display starts at 0,0), e.g. to pick a Retina display.
    pub(crate) position: Option<(f64, f64)>,
    /// Contents of the file named by `ATRIUM_DEV_INIT_SCRIPT`, appended to every service view's
    /// init script.
    pub(crate) init_script: Option<String>,
    /// `ATRIUM_DEV_UI_QUERY`: query string for the sidebar UI, e.g. `dialog=accounts&edit=0`.
    pub(crate) ui_query: Option<String>,
    /// `ATRIUM_LOG_TITLES=1`: log page titles (diagnosing unread title patterns).
    pub(crate) log_titles: bool,
    /// `ATRIUM_DEV_RELEASE_DIR`: folder with `release.json`, `SHA256SUMS` and the asset that
    /// stands in for the latest GitHub release; the debug build then updates itself.
    pub(crate) release_dir: Option<PathBuf>,
}

impl DevHooks {
    #[cfg(debug_assertions)]
    pub(crate) fn from_env() -> DevHooks {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        DevHooks {
            config_dir: var("ATRIUM_CONFIG_DIR").map(Into::into),
            always_on_top: var("ATRIUM_ALWAYS_ON_TOP").is_some_and(|value| value == "1"),
            position: var("ATRIUM_WINDOW_POSITION").and_then(|value| {
                let (x, y) = value.split_once(',')?;
                Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
            }),
            init_script: var("ATRIUM_DEV_INIT_SCRIPT").map(|path| {
                std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("ATRIUM_DEV_INIT_SCRIPT {path}: {error}"))
            }),
            ui_query: var("ATRIUM_DEV_UI_QUERY"),
            log_titles: var("ATRIUM_LOG_TITLES").is_some_and(|value| value == "1"),
            release_dir: var("ATRIUM_DEV_RELEASE_DIR").map(Into::into),
        }
    }

    /// Release builds ignore the environment.
    #[cfg(not(debug_assertions))]
    pub(crate) fn from_env() -> DevHooks {
        DevHooks::default()
    }

    /// Sidebar UI page, with the query string if one is set.
    pub(crate) fn ui_url(&self) -> WebviewUrl {
        match &self.ui_query {
            Some(query) => {
                WebviewUrl::App(format!("index.html?{}", query.trim_start_matches('?')).into())
            }
            None => WebviewUrl::App("index.html".into()),
        }
    }
}
