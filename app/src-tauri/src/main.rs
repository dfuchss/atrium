//! Composition root: builds the core, the Tauri adapter and the file store, and runs the
//! actor thread that owns [`domain::Core`]. Every input (UI commands, native hooks, window
//! events, ticks) goes through one channel; effects are applied in order by the actor.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use domain::ports::{Platform, Store};
use domain::runtime::{Ports, apply};
use domain::{Catalog, Core, Effect, Input, Persisted};
use host_tauri::{Feed, TauriHost};
use protocol::{AppInfo, Snapshot, UI_WEBVIEW, UiCommand};
use store_fs::FsStore;
use tauri::window::{Color, WindowBuilder};
use tauri::{
    App, AppHandle, LogicalPosition, LogicalSize, Manager, RunEvent, State, Theme, TitleBarStyle,
    WebviewBuilder, WebviewUrl, Window, WindowEvent, Wry,
};
use tauri_plugin_autostart::MacosLauncher;

const MAIN_WINDOW: &str = "main";

/// Sidebar background (`--bg` in `app/ui/styles.css`), shown through the transparent title bar.
const SIDEBAR_BG_DARK: Color = Color(0x1b, 0x1c, 0x1f, 0xff);
const SIDEBAR_BG_LIGHT: Color = Color(0xf1, 0xf2, 0xf4, 0xff);

/// Window background for `theme`: the transparent title bar shows it, so it matches the sidebar.
fn window_background(theme: Theme) -> Color {
    if theme == Theme::Dark {
        SIDEBAR_BG_DARK
    } else {
        SIDEBAR_BG_LIGHT
    }
}

/// Development hooks, read from environment variables in debug builds only (README
/// "Development"). Used to run a second, isolated instance with demo data, e.g. for screenshots.
#[derive(Default)]
struct DevHooks {
    /// `ATRIUM_CONFIG_DIR`: config folder (store, services folder, log) instead of the app's.
    config_dir: Option<PathBuf>,
    /// `ATRIUM_ALWAYS_ON_TOP=1`: keep the main window on top (occluded windows are not painted).
    always_on_top: bool,
    /// `ATRIUM_WINDOW_POSITION=<x>,<y>`: main window position in logical px (global screen
    /// coordinates, the main display starts at 0,0), e.g. to pick a Retina display.
    position: Option<(f64, f64)>,
    /// Contents of the file named by `ATRIUM_DEV_INIT_SCRIPT`, appended to every service view's
    /// init script.
    init_script: Option<String>,
    /// `ATRIUM_DEV_UI_QUERY`: query string for the sidebar UI, e.g. `dialog=accounts&edit=0`.
    ui_query: Option<String>,
}

impl DevHooks {
    #[cfg(debug_assertions)]
    fn from_env() -> DevHooks {
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
        }
    }

    /// Release builds ignore the environment.
    #[cfg(not(debug_assertions))]
    fn from_env() -> DevHooks {
        DevHooks::default()
    }

    /// Sidebar UI page, with the query string if one is set.
    fn ui_url(&self) -> WebviewUrl {
        match &self.ui_query {
            Some(query) => {
                WebviewUrl::App(format!("index.html?{}", query.trim_start_matches('?')).into())
            }
            None => WebviewUrl::App("index.html".into()),
        }
    }
}

/// State shared with the Tauri commands.
struct Shared {
    feed: Feed,
    /// Last snapshot emitted by the core; served to the UI on (re)load.
    snapshot: Arc<Mutex<Snapshot>>,
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]
fn ui_command(state: State<'_, Shared>, command: UiCommand) {
    (state.feed)(Input::Ui(command));
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive their arguments by value"
)]
fn get_snapshot(state: State<'_, Shared>) -> Snapshot {
    state
        .snapshot
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn load_persisted(store: &dyn Store, log: &dyn Platform) -> Persisted {
    store.load().unwrap_or_else(|error| {
        log.log(&format!("cannot load accounts, starting empty: {error}"));
        Persisted::default()
    })
}

/// Wall clock in ms since the Unix epoch (ticks; a jump > 30 s means the Mac slept).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| {
            u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Owns the core; never returns while the channel is open.
fn run_actor(
    mut core: Core,
    inputs: Receiver<Input>,
    host: &TauriHost<Wry>,
    store: &FsStore,
    cache: &Mutex<Snapshot>,
) {
    let ports = Ports {
        host,
        notifier: host,
        platform: host,
        ui: host,
        store,
    };
    for input in inputs {
        for effect in core.handle(input) {
            if let Effect::EmitSnapshot(snapshot) = &effect {
                *cache.lock().unwrap_or_else(PoisonError::into_inner) = snapshot.clone();
            }
            apply(effect, &ports);
        }
    }
}

/// Shows, unminimizes and focuses the main window (dock icon click while hidden).
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_window(MAIN_WINDOW) {
        // Nothing sensible to do if the window cannot come to the front.
        let _ = host_tauri::bring_to_front(&window);
    }
}

/// The Tauri host, with the development overrides applied.
fn create_host(app: &App, dev: &DevHooks, feed: Feed) -> TauriHost<Wry> {
    let mut host = TauriHost::new(app.handle().clone(), MAIN_WINDOW, feed);
    if let Some(dir) = &dev.config_dir {
        host = host.with_config_dir(dir.clone());
    }
    if let Some(script) = &dev.init_script {
        host = host.with_extra_init_script(script.clone());
    }
    host
}

/// The app's config folder, or the development override.
fn config_dir(app: &App, dev: &DevHooks, host: &TauriHost<Wry>) -> tauri::Result<PathBuf> {
    match &dev.config_dir {
        Some(dir) => {
            host.log(&format!("development config folder {}", dir.display()));
            Ok(dir.clone())
        }
        None => app.path().app_config_dir(),
    }
}

/// Transparent title bar without a title: it shows the window background (the sidebar color)
/// instead of a lighter strip. Not Overlay: the content view stays below the title bar, so the
/// services' top-edge controls keep working.
fn build_main_window(app: &App, dev: &DevHooks, start_hidden: bool) -> tauri::Result<Window> {
    let mut builder = WindowBuilder::new(app, MAIN_WINDOW)
        .title("Atrium")
        .title_bar_style(TitleBarStyle::Transparent)
        .hidden_title(true)
        .inner_size(1280.0, 800.0)
        .min_inner_size(600.0, 400.0)
        .visible(!start_hidden)
        .always_on_top(dev.always_on_top);
    if let Some((x, y)) = dev.position {
        builder = builder.position(x, y);
    }
    let window = builder.build()?;
    window.set_background_color(Some(window_background(
        window.theme().unwrap_or(Theme::Light),
    )))?;
    Ok(window)
}

/// Adds the sidebar (`ui`) webview at the left edge of `window`.
fn add_sidebar(window: &Window, dev: &DevHooks, width: f64, height: f64) -> tauri::Result<()> {
    window.add_child(
        WebviewBuilder::new(UI_WEBVIEW, dev.ui_url()),
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(width, height),
    )?;
    Ok(())
}

/// Feeds a tick every second (drives polling, hibernation and wake detection).
fn spawn_ticker(feed: Feed) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("ticker".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                feed(Input::Tick { now_ms: now_ms() });
            }
        })?;
    Ok(())
}

fn feed_resize(feed: &Feed, size: LogicalSize<f64>) {
    feed(Input::WindowResized {
        width: size.width,
        height: size.height,
    });
}

/// Closing the window (red button, Cmd+W) hides it or quits, as the settings say; Quit
/// (Cmd+Q) is a menu item (see `host_tauri::graceful_quit`).
fn close_requested(window: &Window, snapshot_cache: &Mutex<Snapshot>) {
    let to_background = snapshot_cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .settings
        .close_to_background;
    if to_background {
        let _ = window.hide();
    } else {
        // Keep the window (and its service views) until the pages are unloaded.
        host_tauri::graceful_quit(window.app_handle());
    }
}

/// Window events of the main window only: popup and app windows opened by services are normal
/// windows that really close.
fn watch_main_window(window: &Window, feed: Feed, snapshot_cache: Arc<Mutex<Snapshot>>) {
    let event_window = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            close_requested(&event_window, &snapshot_cache);
        }
        WindowEvent::ThemeChanged(theme) => {
            let _ = event_window.set_background_color(Some(window_background(*theme)));
        }
        WindowEvent::Resized(size) => {
            // A Resized event reports the new physical size; the scale factor is unchanged
            // (ScaleFactorChanged carries its own).
            if let Ok(scale) = event_window.scale_factor() {
                feed_resize(&feed, size.to_logical::<f64>(scale));
            }
        }
        WindowEvent::ScaleFactorChanged {
            scale_factor,
            new_inner_size,
            ..
        } => feed_resize(&feed, new_inner_size.to_logical::<f64>(*scale_factor)),
        _ => {}
    });
}

/// Builds the window, the host and the core, starts the actor and the ticker, and feeds
/// `Input::Started`. The host only needs the window lazily, so it can log while loading the
/// settings that decide how the window is created.
fn setup(
    app: &App,
    feed: &Feed,
    inputs: Receiver<Input>,
    snapshot_cache: &Arc<Mutex<Snapshot>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let dev = DevHooks::from_env();
    let host = create_host(app, &dev, feed.clone());
    let version = app.package_info().version.to_string();
    host.log(&format!("---- Atrium {version} starting ----"));
    let config_dir = config_dir(app, &dev, &host)?;
    let store = FsStore::new(config_dir.clone());
    let persisted = load_persisted(&store, &host);
    let services_dir = config_dir.join("services");
    let (user_defs, user_errors) = host_tauri::services::read_user_services(&services_dir);
    host.log(&format!(
        "config {}, {} accounts, {} user services, {} service errors",
        config_dir.display(),
        persisted.accounts.len(),
        user_defs.len(),
        user_errors.len()
    ));

    let window = build_main_window(app, &dev, persisted.settings.app.start_hidden)?;
    let size = window
        .inner_size()?
        .to_logical::<f64>(window.scale_factor()?);
    add_sidebar(
        &window,
        &dev,
        persisted.settings.app.icon_size.sidebar_width(),
        size.height,
    )?;

    let mut core = Core::new(Catalog::builtin(), persisted, Box::new(uuid::Uuid::new_v4));
    core.set_user_services(user_defs, user_errors);
    core.set_app_info(AppInfo {
        version,
        config_dir: config_dir.display().to_string(),
        services_dir: services_dir.display().to_string(),
        log_file: host
            .log_file()
            .map_or_default(|path| path.display().to_string()),
    });
    *snapshot_cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = core.snapshot();

    // The actor may block in `add_child` until the main thread runs the build, so the main
    // thread (this function, window events, commands) only ever sends to it.
    let actor_cache = snapshot_cache.clone();
    std::thread::Builder::new()
        .name("core".into())
        .spawn(move || run_actor(core, inputs, &host, &store, &actor_cache))?;
    spawn_ticker(feed.clone())?;
    watch_main_window(&window, feed.clone(), snapshot_cache.clone());
    feed(Input::Started {
        width: size.width,
        height: size.height,
    });
    Ok(())
}

fn main() {
    let context = tauri::generate_context!();
    let (sender, inputs) = mpsc::channel::<Input>();
    let feed: Feed = Arc::new(move |input| {
        // Fails only after the actor is gone (app shutting down).
        let _ = sender.send(input);
    });
    let snapshot_cache = Arc::new(Mutex::new(Snapshot::default()));
    let (menu_feed, setup_feed, setup_cache) = (feed.clone(), feed.clone(), snapshot_cache.clone());

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(Shared {
            feed,
            snapshot: snapshot_cache,
        })
        .invoke_handler(tauri::generate_handler![ui_command, get_snapshot])
        // App menu and tray menu items (built by the host from the core's menu model).
        .on_menu_event(move |app, event| {
            host_tauri::handle_menu_event(app, MAIN_WINDOW, event.id().as_ref(), &menu_feed);
        })
        .setup(move |app| setup(app, &setup_feed, inputs, &setup_cache))
        .build(context)
        .expect("error while building Atrium");
    app.run(|app, event| match event {
        // Dock icon click: bring a hidden (closed to background / started hidden) window back.
        RunEvent::Reopen { .. } => show_main_window(app),
        // Any other exit request (e.g. the last window went away) unloads the pages first; the
        // request `graceful_quit` ends with passes. The predefined Quit (`terminate:`, still used
        // by the Dock menu and at logout) skips this event, hence the custom Quit menu items.
        RunEvent::ExitRequested { api, .. } if !host_tauri::is_quitting() => {
            api.prevent_exit();
            host_tauri::graceful_quit(app);
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    /// Service webviews are children of the main window, so a window pattern in the UI
    /// capability would grant them IPC as soon as only their origin stood in the way.
    #[test]
    fn ui_capability_targets_only_the_ui_webview() {
        let capability: Value = serde_json::from_str(include_str!("../capabilities/ui.json"))
            .expect("capability is valid JSON");
        assert!(capability.get("windows").is_none());
        assert_eq!(
            capability["webviews"],
            serde_json::json!([super::UI_WEBVIEW])
        );
    }
}
