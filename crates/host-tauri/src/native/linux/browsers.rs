//! The browser for external links: `AppSettings.browser` holds the desktop entry id of an
//! installed web browser (e.g. `firefox.desktop`), chosen from the apps that handle `https` links.
//! `mailto:` links and links without a usable choice go to the desktop's default app.
//!
//! Apps are launched through GIO with the environment Atrium was started with: from an
//! `AppImage`, Atrium's own carries the `AppImage`'s libraries, GTK and `GStreamer` modules and
//! data folders, which would break or confuse the launched browser ([`launch_environment`]).
//! Its working directory is the `AppImage`'s `usr` folder too, which its runtime changes to and
//! Atrium keeps: the bundled `WebKitGTK` finds its processes relative to it. Launched apps start in
//! the home folder instead ([`in_home_folder`]).

use std::env;
use std::path::{Path, PathBuf};

use gtk::gio::DesktopAppInfo;
use gtk::glib::prelude::Cast;
use gtk::glib::{KeyFile, KeyFileFlags};

use domain::ports::PortResult;
use gtk::gio::prelude::{AppInfoExt, AppLaunchContextExt};
use gtk::gio::{AppInfo, AppLaunchContext};
use protocol::BrowserChoice;
use tauri::{AppHandle, Runtime};
use url::Url;

use super::theme::GTK_THEME;
use crate::{Logger, port_err};

const HTTPS_HANDLER: &str = "x-scheme-handler/https";

/// Atrium's own desktop entry, named after its app id (see `set_app_id`), never offered.
const ATRIUM_ENTRY: &str = "org.fuchss.atrium.desktop";

const DESKTOP_ENTRY_GROUP: &str = "Desktop Entry";
/// The key of a desktop entry that names the working directory of the app.
const WORKING_DIRECTORY_KEY: &str = "Path";

/// Variables of the `AppImage` runtime and of its GTK and `GStreamer` hooks that hold no path
/// under `$APPDIR`, which [`launch_environment`] would recognise.
const APPIMAGE_VARIABLES: [&str; 6] = [
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
    GTK_THEME,
    "GST_REGISTRY_REUSE_PLUGIN_SCANNER",
];

/// Opens `url` with the installed browser `browser` (a desktop entry id), or with the default
/// app for its scheme: always for `mailto:`, and when the browser is not installed (logged).
pub(crate) fn open_url_with<R: Runtime>(
    _app: &AppHandle<R>,
    url: &str,
    browser: Option<&str>,
    logger: &Logger,
) -> PortResult {
    let chosen = browser.filter(|_| !is_mail_link(url)).and_then(|id| {
        installed_browser(id)
            .inspect_err(|reason| logger.log(&format!("browser {id} {reason}, using the default")))
            .ok()
    });
    let context = launch_context();
    match chosen.or_else(|| default_app(url)) {
        Some(app) => in_home_folder(app).launch_uris(&[url], Some(&context)),
        // Reports that no app opens `url`.
        None => AppInfo::launch_default_for_uri(url, Some(&context)),
    }
    .map_err(port_err)
}

/// The desktop's default app for the scheme of `url`.
fn default_app(url: &str) -> Option<AppInfo> {
    AppInfo::default_for_uri_scheme(Url::parse(url).ok()?.scheme())
}

/// `app` set to start in the home folder when Atrium runs as an `AppImage`. GIO takes the working
/// directory only from the `Path` key of a desktop entry, so this is a copy of the app's entry
/// with that key, unless the entry names one itself; `app` as it is if its entry cannot be read.
fn in_home_folder(app: AppInfo) -> AppInfo {
    if appimage_dirs().is_none() {
        return app;
    }
    let home = gtk::glib::home_dir();
    with_working_directory(&app, &home).unwrap_or(app)
}

/// A copy of `app`'s desktop entry that starts in `directory`; None if the entry names its own
/// working directory or cannot be read.
fn with_working_directory(app: &AppInfo, directory: &Path) -> Option<AppInfo> {
    let entry = KeyFile::new();
    let file = app.downcast_ref::<DesktopAppInfo>()?.filename()?;
    entry.load_from_file(file, KeyFileFlags::NONE).ok()?;
    let own = entry
        .string(DESKTOP_ENTRY_GROUP, WORKING_DIRECTORY_KEY)
        .ok();
    let directory = launch_directory(own.as_deref(), directory)?;
    entry.set_string(
        DESKTOP_ENTRY_GROUP,
        WORKING_DIRECTORY_KEY,
        directory.to_str()?,
    );
    Some(DesktopAppInfo::from_keyfile(&entry)?.upcast())
}

/// The working directory to set for an entry whose own is `own`: `home`, unless the entry names
/// one.
fn launch_directory<'home>(own: Option<&str>, home: &'home Path) -> Option<&'home Path> {
    own.is_none_or(str::is_empty).then_some(home)
}

fn is_mail_link(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| url.scheme() == "mailto")
}

/// The offered browser `id` if it takes links, else why it cannot open one.
fn installed_browser(id: &str) -> Result<AppInfo, &'static str> {
    let browser = browser_apps()
        .find(|app| app.id().is_some_and(|app_id| app_id == id) && id != ATRIUM_ENTRY)
        .ok_or("is not an installed browser")?;
    if browser.supports_uris() {
        Ok(browser)
    } else {
        Err("cannot open links")
    }
}

/// The apps that handle `https` links, by name.
#[expect(
    clippy::unnecessary_wraps,
    reason = "same function as on macOS, which has no list"
)]
pub(crate) fn installed_browsers() -> Option<Vec<BrowserChoice>> {
    let apps = browser_apps().filter_map(|app| {
        Some(BrowserChoice {
            id: app.id()?.to_string(),
            name: app.name().to_string(),
        })
    });
    Some(offered(apps))
}

/// The apps that handle `https` links and are meant to be shown in menus.
fn browser_apps() -> impl Iterator<Item = AppInfo> {
    AppInfo::all_for_type(HTTPS_HANDLER)
        .into_iter()
        .filter(AppInfo::should_show)
}

/// A launch context with Atrium's environment cleaned by [`launch_environment`] when it runs
/// as an `AppImage`; variables that are no UTF-8 are passed on as they are.
fn launch_context() -> AppLaunchContext {
    let context = AppLaunchContext::new();
    let Some(appdirs) = appimage_dirs() else {
        return context;
    };
    let environment: Vec<(String, String)> = env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect();
    for (name, _) in &environment {
        context.unsetenv(name);
    }
    for (name, value) in launch_environment(environment, &appdirs) {
        context.setenv(name, value);
    }
    context
}

/// The folder the running `AppImage` is mounted at, as given and resolved; None outside an
/// `AppImage`.
fn appimage_dirs() -> Option<Vec<PathBuf>> {
    env::var_os("APPIMAGE")?;
    let appdir = PathBuf::from(env::var_os("APPDIR").filter(|dir| !dir.is_empty())?);
    let resolved = appdir.canonicalize().ok();
    Some(std::iter::once(appdir).chain(resolved).collect())
}

/// The environment for an app launched from the `AppImage` mounted at `appdirs`, from Atrium's
/// `environment`: without the [`APPIMAGE_VARIABLES`] and the entries of path lists that lie in
/// the `AppImage` (`PATH`, `LD_LIBRARY_PATH`, `XDG_DATA_DIRS`, `GIO_MODULE_DIR`,
/// `GST_PLUGIN_PATH_1_0`, ...), which its runtime and hooks prepend, and the `PWD` and `OLDPWD`
/// that its runtime's change of directory left; a variable left without an entry is dropped.
fn launch_environment(
    environment: Vec<(String, String)>,
    appdirs: &[PathBuf],
) -> Vec<(String, String)> {
    environment
        .into_iter()
        .filter(|(name, _)| !APPIMAGE_VARIABLES.contains(&name.as_str()))
        .filter_map(|(name, value)| Some((name, outside_appimage(&value, appdirs)?)))
        .collect()
}

/// `value` without its `:`-separated entries in `appdirs` (and then without empty entries);
/// None if no entry is left.
fn outside_appimage(value: &str, appdirs: &[PathBuf]) -> Option<String> {
    let inside = |entry: &&str| appdirs.iter().any(|dir| Path::new(entry).starts_with(dir));
    if !value.split(':').any(|entry| inside(&entry)) {
        return Some(value.to_string());
    }
    let kept: Vec<&str> = value
        .split(':')
        .filter(|entry| !entry.is_empty() && !inside(entry))
        .collect();
    (!kept.is_empty()).then(|| kept.join(":"))
}

/// Drops Atrium and repeated ids, and sorts by name.
fn offered(choices: impl IntoIterator<Item = BrowserChoice>) -> Vec<BrowserChoice> {
    let mut offered: Vec<BrowserChoice> = Vec::new();
    for choice in choices {
        if choice.id != ATRIUM_ENTRY && offered.iter().all(|known| known.id != choice.id) {
            offered.push(choice);
        }
    }
    offered.sort_by_key(|choice| choice.name.to_lowercase());
    offered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(id: &str, name: &str) -> BrowserChoice {
        BrowserChoice {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn offers_other_apps_by_name_without_duplicates_or_atrium() {
        let list = offered([
            choice("firefox.desktop", "Firefox"),
            choice(ATRIUM_ENTRY, "Atrium"),
            choice("brave.desktop", "brave"),
            choice("firefox.desktop", "Firefox"),
        ]);
        let ids: Vec<&str> = list.iter().map(|choice| choice.id.as_str()).collect();
        assert_eq!(ids, ["brave.desktop", "firefox.desktop"]);
    }

    fn variables(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn launched_apps_get_no_appimage_environment() {
        let appdirs = [
            PathBuf::from("/tmp/.mount_AtriumX"),
            PathBuf::from("/private/tmp/.mount_AtriumX"),
        ];
        let atrium = variables(&[
            ("APPDIR", "/tmp/.mount_AtriumX"),
            ("APPIMAGE", "/home/me/Atrium.AppImage"),
            ("ARGV0", "Atrium.AppImage"),
            ("OWD", "/home/me"),
            ("GTK_THEME", "Adwaita:dark"),
            ("GST_REGISTRY_REUSE_PLUGIN_SCANNER", "no"),
            ("GTK_DATA_PREFIX", "/tmp/.mount_AtriumX"),
            (
                "GDK_PIXBUF_MODULE_FILE",
                "/tmp/.mount_AtriumX/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache",
            ),
            (
                "PATH",
                "/private/tmp/.mount_AtriumX/usr/bin/:/usr/local/bin:/usr/bin",
            ),
            (
                "LD_LIBRARY_PATH",
                "/tmp/.mount_AtriumX/usr/lib/:/tmp/.mount_AtriumX/lib/:",
            ),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_AtriumX/usr/share:/usr/share:/home/me/.local/share/flatpak/exports/share",
            ),
            ("GTK_IM_MODULE", "ibus"),
            ("HOME", "/home/me"),
            ("EMPTY", ""),
            ("OTHER_MOUNT", "/tmp/.mount_AtriumXY/usr/lib"),
            ("PWD", "/tmp/.mount_AtriumX/usr"),
            ("OLDPWD", "/tmp/.mount_AtriumX/usr"),
        ]);
        assert_eq!(
            launch_environment(atrium, &appdirs),
            variables(&[
                ("PATH", "/usr/local/bin:/usr/bin"),
                (
                    "XDG_DATA_DIRS",
                    "/usr/share:/home/me/.local/share/flatpak/exports/share"
                ),
                ("GTK_IM_MODULE", "ibus"),
                ("HOME", "/home/me"),
                ("EMPTY", ""),
                ("OTHER_MOUNT", "/tmp/.mount_AtriumXY/usr/lib"),
            ])
        );
    }

    #[test]
    fn launched_apps_start_at_home_unless_their_entry_names_a_folder() {
        let home = Path::new("/home/me");
        assert_eq!(launch_directory(None, home), Some(home));
        assert_eq!(launch_directory(Some(""), home), Some(home));
        assert_eq!(launch_directory(Some("/opt/browser"), home), None);
    }

    #[test]
    fn mail_links_are_told_apart() {
        assert!(is_mail_link("mailto:a@b.org"));
        assert!(is_mail_link("MAILTO:a@b.org"));
        assert!(!is_mail_link("https://example.org/mailto:a@b.org"));
    }
}
