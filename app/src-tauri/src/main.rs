//! Composition root: builds the core, the Tauri adapter and the file store, and runs the
//! actor thread that owns [`domain::Core`]. Every input (UI commands, native hooks, window
//! events, ticks) goes through one channel; effects are applied in order by the actor.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod updates;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use domain::ports::{Platform, Store};
use domain::runtime::{Ports, StartupLoad, apply};
use domain::{Bounds, Catalog, Core, Effect, Input, Persisted};
use host_tauri::{Feed, TauriHost};
#[cfg(target_os = "macos")]
use macos_dialogs::MainThreadMarker;
use protocol::{AppInfo, AppSettings, Snapshot, UI_WEBVIEW, UiCommand};
use store_fs::FsStore;
#[cfg(target_os = "macos")]
use tauri::TitleBarStyle;
use tauri::window::WindowBuilder;
use tauri::{
    App, AppHandle, LogicalSize, Manager, RunEvent, State, Theme, WebviewBuilder, WebviewUrl,
    Window, WindowEvent, Wry,
};
use tauri_plugin_autostart::MacosLauncher;
#[cfg(not(target_os = "macos"))]
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

mod service_files;

const MAIN_WINDOW: &str = "main";

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
    /// `ATRIUM_LOG_TITLES=1`: log page titles (diagnosing unread title patterns).
    log_titles: bool,
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
            log_titles: var("ATRIUM_LOG_TITLES").is_some_and(|value| value == "1"),
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

/// The saved accounts and settings. If the file cannot be read, the run is read-only: nothing
/// is saved (the file may still hold the real accounts) and the user is told once.
fn load_persisted(store: &dyn Store, log: &dyn Platform) -> StartupLoad {
    let load = StartupLoad::from_store(store.load());
    if let StartupLoad::Unreadable(error) = &load {
        log.log(&format!(
            "cannot read accounts, running read-only and not saving: {error}"
        ));
    }
    load
}

const UNREADABLE_SETTINGS_TITLE: &str = "Atrium cannot read its settings";
const UNREADABLE_SETTINGS_MESSAGE: &str = "Your accounts could not be loaded, so Atrium started empty. Changes made in this session will not be saved, and your saved accounts stay untouched. Quit and check the permissions of accounts.json, then start Atrium again.";

/// Tells the user that this session is read-only; the UI cannot carry this because it starts
/// after the load. `setup` runs on the main thread, where `run_on_main_thread` runs the alert
/// inline: its modal loop holds the start until the user confirms, before the (empty) window
/// exists, which is intended.
#[cfg(target_os = "macos")]
fn show_unreadable_settings_alert(app: &AppHandle) {
    let shown = app.run_on_main_thread(|| {
        if let Some(main_thread) = MainThreadMarker::new() {
            macos_dialogs::show_warning(
                main_thread,
                UNREADABLE_SETTINGS_TITLE,
                UNREADABLE_SETTINGS_MESSAGE,
            );
        }
    });
    // The alert is a courtesy; the log already holds the reason.
    drop(shown);
}

/// Like on macOS, but without holding the start: waiting for the answer on the main thread
/// would block the event loop that shows the dialog.
#[cfg(not(target_os = "macos"))]
fn show_unreadable_settings_alert(app: &AppHandle) {
    app.dialog()
        .message(UNREADABLE_SETTINGS_MESSAGE)
        .title(UNREADABLE_SETTINGS_TITLE)
        .kind(MessageDialogKind::Warning)
        .show(drop);
}

/// Deletes the web data of accounts that no longer exist, but only against an intact
/// `accounts.json` of the regular installation (`allowed`: loaded, no development config
/// folder, which shares the app's data stores without listing its accounts). Stores of
/// accounts added meanwhile are kept: `snapshot_cache` holds the live accounts.
fn remove_unused_data_stores(
    host: &TauriHost<Wry>,
    store: &FsStore,
    persisted: &Persisted,
    allowed: bool,
    snapshot_cache: &Arc<Mutex<Snapshot>>,
) {
    if allowed && store.lists_all_accounts() {
        host.remove_unused_data_stores(persisted.accounts.clone(), snapshot_cache.clone());
    } else {
        host.log("not removing unused data stores: no complete, intact account list");
    }
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
fn run_actor(mut core: Core, inputs: Receiver<Input>, ports: &Ports<'_>, cache: &Mutex<Snapshot>) {
    for input in inputs {
        for effect in core.handle(input) {
            if let Effect::EmitSnapshot(snapshot) = &effect {
                *cache.lock().unwrap_or_else(PoisonError::into_inner) = snapshot.clone();
            }
            apply(effect, ports);
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

/// On macOS a transparent title bar without a title: it shows the window background (the
/// sidebar color) instead of a lighter strip. Not Overlay: the content view stays below the
/// title bar, so the services' top-edge controls keep working. Elsewhere the system decorations.
fn build_main_window(app: &App, dev: &DevHooks, start_hidden: bool) -> tauri::Result<Window> {
    let builder = WindowBuilder::new(app, MAIN_WINDOW).title("Atrium");
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(TitleBarStyle::Transparent)
        .hidden_title(true);
    let mut builder = builder
        .inner_size(1280.0, 800.0)
        .min_inner_size(600.0, 400.0)
        .visible(!start_hidden)
        .always_on_top(dev.always_on_top);
    if let Some((x, y)) = dev.position {
        builder = builder.position(x, y);
    }
    let window = builder.build()?;
    window.set_background_color(Some(host_tauri::window_background(
        window.theme().unwrap_or(Theme::Light),
    )))?;
    Ok(window)
}

/// Adds the sidebar (`ui`) webview at the left edge of `window`.
fn add_sidebar(window: &Window, dev: &DevHooks, width: f64, height: f64) -> tauri::Result<()> {
    let bounds = Bounds {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    host_tauri::add_child(
        window,
        WebviewBuilder::new(UI_WEBVIEW, dev.ui_url()),
        bounds,
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
fn watch_main_window(
    window: &Window,
    feed: Feed,
    snapshot_cache: Arc<Mutex<Snapshot>>,
) -> tauri::Result<()> {
    host_tauri::watch_content_size(window, move |width, height| {
        feed(Input::WindowResized { width, height });
    })?;
    let event_window = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            close_requested(&event_window, &snapshot_cache);
        }
        WindowEvent::ThemeChanged(theme) => {
            let _ = event_window.set_background_color(Some(host_tauri::window_background(*theme)));
        }
        _ => {}
    });
    Ok(())
}

/// What the actor thread owns besides the core.
struct ActorEnv {
    host: TauriHost<Wry>,
    store: FsStore,
    cache: Arc<Mutex<Snapshot>>,
    /// Saved data was unreadable: do not persist.
    read_only: bool,
}

/// Runs the actor on its own thread, which owns the core, the host and the store.
fn spawn_actor(core: Core, inputs: Receiver<Input>, env: ActorEnv) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("core".into())
        .spawn(move || {
            let ports = Ports {
                host: &env.host,
                notifier: &env.host,
                platform: &env.host,
                ui: &env.host,
                store: &env.store,
                read_only: env.read_only,
            };
            run_actor(core, inputs, &ports, &env.cache);
        })?;
    Ok(())
}

/// Creates the main window, docks the sidebar webview at its full height and returns the
/// window with its logical size.
fn build_window_with_sidebar(
    app: &App,
    dev: &DevHooks,
    app_settings: &AppSettings,
) -> tauri::Result<(Window, LogicalSize<f64>)> {
    let window = build_main_window(app, dev, app_settings.start_hidden)?;
    let size = window
        .inner_size()?
        .to_logical::<f64>(window.scale_factor()?);
    add_sidebar(
        &window,
        dev,
        app_settings.icon_size.sidebar_width(),
        size.height,
    )?;
    Ok((window, size))
}

/// Builds the window, the host and the core, starts the actor and the ticker, and feeds
/// `Input::Started`. The host only needs the window lazily, so it can log while loading the
/// settings that decide how the window is created.
fn setup(
    app: &App,
    dev: &DevHooks,
    feed: &Feed,
    inputs: Receiver<Input>,
    snapshot_cache: &Arc<Mutex<Snapshot>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let host = create_host(app, dev, feed.clone());
    let version = app.package_info().version.to_string();
    host.log(&format!("---- Atrium {version} starting ----"));
    let config_dir = config_dir(app, dev, &host)?;
    let store = FsStore::new(config_dir.clone());
    updates::start(app, config_dir.clone(), host.log_sink())?;
    let load = load_persisted(&store, &host);
    let read_only = load.is_read_only();
    if read_only {
        show_unreadable_settings_alert(app.handle());
    }
    let persisted = load.into_persisted();
    let sweep_allowed = !read_only && dev.config_dir.is_none();
    remove_unused_data_stores(&host, &store, &persisted, sweep_allowed, snapshot_cache);
    let services_dir = config_dir.join("services");
    app.manage(service_files::ServiceFiles {
        dir: services_dir.clone(),
        feed: feed.clone(),
    });
    let (user_defs, user_errors) = host_tauri::services::read_user_services(&services_dir);
    host.log(&format!(
        "config {}, {} accounts, {} user services, {} service errors",
        config_dir.display(),
        persisted.accounts.len(),
        user_defs.len(),
        user_errors.len()
    ));

    // Before the window exists, so a forced theme doesn't flash the system one at start.
    app.set_theme(host_tauri::native_theme(persisted.settings.app.theme));
    let (window, size) = build_window_with_sidebar(app, dev, &persisted.settings.app)?;
    // Needs the sidebar webview and must precede the service views: see `install_dialogs`.
    host.install_dialogs();

    let mut core = Core::new(Catalog::builtin(), persisted, Box::new(uuid::Uuid::new_v4));
    core.set_user_services(user_defs, user_errors);
    core.set_log_page_titles(dev.log_titles);
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
    let env = ActorEnv {
        host,
        store,
        cache: snapshot_cache.clone(),
        read_only,
    };
    spawn_actor(core, inputs, env)?;
    spawn_ticker(feed.clone())?;
    watch_main_window(&window, feed.clone(), snapshot_cache.clone())?;
    feed(Input::Started {
        width: size.width,
        height: size.height,
    });
    Ok(())
}

/// The plugins; the development hooks decide about single-instance (not on macOS).
#[cfg_attr(
    target_os = "macos",
    expect(unused_variables, reason = "macOS keeps one instance itself")
)]
fn with_plugins(builder: tauri::Builder<Wry>, dev: &DevHooks) -> tauri::Builder<Wry> {
    // macOS keeps one instance itself and reports a second start as `Reopen`. A development
    // instance runs next to the regular one.
    #[cfg(not(target_os = "macos"))]
    let builder = if dev.config_dir.is_none() {
        builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main_window(app);
        }))
    } else {
        builder
    }
    .plugin(tauri_plugin_dialog::init());
    builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
}

fn main() {
    // Started as the update helper (see `updater::helper`): swap the bundle, never open a window.
    if let Some(exit_code) = updater::helper::run_if_requested() {
        std::process::exit(exit_code);
    }
    let context = tauri::generate_context!();
    let (sender, inputs) = mpsc::channel::<Input>();
    let feed: Feed = Arc::new(move |input| {
        // Fails only after the actor is gone (app shutting down).
        let _ = sender.send(input);
    });
    let snapshot_cache = Arc::new(Mutex::new(Snapshot::default()));
    let (menu_feed, setup_feed, setup_cache) = (feed.clone(), feed.clone(), snapshot_cache.clone());
    let dev = DevHooks::from_env();

    let app = with_plugins(tauri::Builder::default(), &dev)
        .manage(Shared {
            feed,
            snapshot: snapshot_cache,
        })
        .invoke_handler(tauri::generate_handler![
            ui_command,
            get_snapshot,
            service_files::list_service_files,
            service_files::read_service_file,
            service_files::new_service_template,
            service_files::validate_service,
            service_files::save_service_file,
            service_files::delete_service_file,
            service_files::set_service_file_enabled,
            updates::update_command,
            updates::get_update_status,
        ])
        // App menu and tray menu items (built by the host from the core's menu model).
        .on_menu_event(move |app, event| {
            host_tauri::handle_menu_event(app, MAIN_WINDOW, event.id().as_ref(), &menu_feed);
        })
        .setup(move |app| setup(app, &dev, &setup_feed, inputs, &setup_cache))
        .build(context)
        .expect("error while building Atrium");
    app.run(|app, event| match event {
        // Dock icon click: bring a hidden (closed to background / started hidden) window back.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main_window(app),
        // "Restart and update": the helper swaps the bundle once this process is gone.
        RunEvent::Exit => updates::launch_pending_helper(app),
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
