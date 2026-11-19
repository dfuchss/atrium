//! Composition root: builds the core, the Tauri adapter and the file store, and runs the
//! actor thread that owns [`domain::Core`]. Every input (UI commands, native hooks, window
//! events, ticks) goes through one channel; effects are applied in order by the actor.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod desktop;
#[cfg(not(windows))]
mod desktop_entry;
mod dev_hooks;
mod main_window;
mod service_files;
mod snapshot_cache;
#[cfg(windows)]
mod start_menu;
mod transfer;
mod updates;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use domain::ports::{Platform, Store};
use domain::runtime::{Ports, StartupLoad, apply};
use domain::{Catalog, Core, Effect, Input, Persisted};
use fs_util::now_ms;
use host_tauri::{Feed, TauriHost};
use protocol::{AppInfo, OperatingSystem, Snapshot, UiCommand};
use store_fs::FsStore;
use tauri::{App, AppHandle, Manager, RunEvent, State, Wry};
use tauri_plugin_autostart::MacosLauncher;

use crate::dev_hooks::DevHooks;
use crate::main_window::{MAIN_WINDOW, build_window_with_sidebar, watch_main_window};
use crate::snapshot_cache::SnapshotCache;

/// State shared with the Tauri commands.
struct Shared {
    feed: Feed,
    /// Last snapshot emitted by the core; served to the UI on (re)load.
    snapshot: SnapshotCache,
}

/// Calls `$then!` with the Tauri commands, each with the protocol constant the UI calls it by:
/// the one list behind the invoke handler and the test of the names.
macro_rules! with_commands {
    ($then:ident) => {
        $then! {
            ui_command => CMD_UI_COMMAND,
            get_snapshot => CMD_GET_SNAPSHOT,
            service_files::list_service_files => CMD_LIST_SERVICE_FILES,
            service_files::read_service_file => CMD_READ_SERVICE_FILE,
            service_files::new_service_template => CMD_NEW_SERVICE_TEMPLATE,
            service_files::validate_service => CMD_VALIDATE_SERVICE,
            service_files::save_service_file => CMD_SAVE_SERVICE_FILE,
            service_files::delete_service_file => CMD_DELETE_SERVICE_FILE,
            service_files::set_service_file_enabled => CMD_SET_SERVICE_FILE_ENABLED,
            transfer::transfer_command => CMD_TRANSFER_COMMAND,
            desktop::get_desktop_entry => CMD_GET_DESKTOP_ENTRY,
            desktop::install_desktop_entry => CMD_INSTALL_DESKTOP_ENTRY,
            desktop::uninstall_desktop_entry => CMD_UNINSTALL_DESKTOP_ENTRY,
            desktop::list_browsers => CMD_LIST_BROWSERS,
            updates::update_command => CMD_UPDATE_COMMAND,
            updates::get_update_status => CMD_GET_UPDATE_STATUS,
        }
    };
}

/// The invoke handler of the commands of [`with_commands`].
macro_rules! handler {
    ($($($segment:ident)::+ => $constant:ident),* $(,)?) => {
        tauri::generate_handler![$($($segment)::+),*]
    };
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
    state.snapshot.get()
}

/// The saved accounts and settings. If the file cannot be read, the run is read-only: nothing
/// is saved (the file may still hold the real accounts) and the user is told once. A corrupt
/// file the store set aside is logged.
fn load_persisted(store: &FsStore, log: &dyn Platform) -> StartupLoad {
    let load = StartupLoad::from_store(store.load());
    if let Some(quarantined) = store.take_quarantined() {
        log.log(&format!("starting without accounts: {quarantined}"));
    }
    if let StartupLoad::Unreadable(error) = &load {
        log.log(&format!(
            "cannot read accounts, running read-only and not saving: {error}"
        ));
    }
    load
}

/// Tells the user that this session is read-only; the UI cannot carry this because it starts
/// after the load. `setup` runs on the main thread, where `run_on_main_thread` runs the alert
/// inline: its modal loop holds the start until the user confirms, before the (empty) window
/// exists, which is intended.
fn show_unreadable_settings_alert(app: &AppHandle) {
    let shown = host_tauri::show_warning(
        app,
        "Atrium cannot read its settings",
        "Your accounts could not be loaded, so Atrium started empty. Changes made in this session will not be saved, and your saved accounts stay untouched. Quit and check the permissions of accounts.json, then start Atrium again.",
    );
    // The alert is a courtesy; the log already holds the reason.
    drop(shown);
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
    snapshot_cache: &SnapshotCache,
) {
    if allowed && store.lists_all_accounts() {
        host.remove_unused_data_stores(persisted.accounts.clone(), snapshot_cache.shared());
    } else {
        host.log("not removing unused data stores: no complete, intact account list");
    }
}

/// Owns the core; never returns while the channel is open. Each snapshot also goes to the
/// snapshot cache and names the accounts in the dialog titles of `host`.
fn run_actor(
    mut core: Core,
    inputs: Receiver<Input>,
    ports: &Ports<'_>,
    host: &TauriHost<Wry>,
    cache: &SnapshotCache,
) {
    for input in inputs {
        for effect in core.handle(input) {
            if let Effect::EmitSnapshot(snapshot) = &effect {
                cache.set(snapshot.clone());
                host.set_dialog_names(snapshot);
            }
            apply(effect, ports);
        }
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

/// Feeds a tick every second (drives polling, hibernation and sleep detection).
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

/// The operating system this build runs on.
fn host_os() -> OperatingSystem {
    if cfg!(target_os = "linux") {
        OperatingSystem::Linux
    } else if cfg!(windows) {
        OperatingSystem::Windows
    } else {
        OperatingSystem::MacOs
    }
}

/// Feeds each return of the user (see [`TauriHost::observe_resume`]); the core handles the
/// views only if the computer slept before.
fn observe_resume(host: &TauriHost<Wry>, feed: Feed) {
    host.observe_resume(move || feed(Input::Resumed { now_ms: now_ms() }));
}

/// What the actor thread owns besides the core.
struct ActorEnv {
    host: TauriHost<Wry>,
    store: FsStore,
    cache: SnapshotCache,
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
            run_actor(core, inputs, &ports, &env.host, &env.cache);
        })?;
    Ok(())
}

/// The facts of the About section.
fn app_info(
    host: &TauriHost<Wry>,
    version: String,
    config_dir: &Path,
    services_dir: &Path,
) -> AppInfo {
    AppInfo {
        os: host_os(),
        version,
        config_dir: config_dir.display().to_string(),
        services_dir: services_dir.display().to_string(),
        log_file: host
            .log_file()
            .map_or_default(|path| path.display().to_string()),
    }
}

/// Builds the window, the host and the core, starts the resume observer, the actor and the
/// ticker, and feeds `Input::Started`. The host only needs the window lazily, so it can log
/// while loading the settings that decide how the window is created.
fn setup(
    app: &App,
    dev: &DevHooks,
    feed: &Feed,
    inputs: Receiver<Input>,
    snapshot_cache: &SnapshotCache,
) -> tauri::Result<()> {
    let host = create_host(app, dev, feed.clone());
    let version = app.package_info().version.to_string();
    host.log(&format!("---- Atrium {version} starting ----"));
    let config_dir = config_dir(app, dev, &host)?;
    let store = FsStore::new(config_dir.clone());
    app.manage(FsStore::new(config_dir.clone()));
    updates::start(app, &config_dir, host.log_sink(), dev.release_dir.clone())?;
    app.manage(desktop::DesktopLog(host.log_sink()));
    let load = load_persisted(&store, &host);
    let read_only = load.is_read_only();
    if read_only {
        show_unreadable_settings_alert(app.handle());
    }
    let persisted = load.into_persisted();
    let sweep_allowed = !read_only && dev.config_dir.is_none();
    remove_unused_data_stores(&host, &store, &persisted, sweep_allowed, snapshot_cache);
    let services_dir = store_fs::services::dir(&config_dir);
    app.manage(service_files::ServiceFiles {
        dir: services_dir.clone(),
        feed: feed.clone(),
    });
    let (user_defs, user_errors) = store_fs::services::read_user_services(&services_dir);
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

    let mut core = Core::new(
        Catalog::builtin(),
        persisted,
        Box::new(uuid::Uuid::new_v4),
        host_tauri::user_agent_platform(),
    );
    core.set_user_services(user_defs, user_errors);
    core.set_log_page_titles(dev.log_titles);
    core.set_new_account_user_agent(host_tauri::new_account_user_agent());
    core.set_page_capture_detection(host_tauri::page_capture_detection());
    core.set_app_info(app_info(&host, version, &config_dir, &services_dir));
    snapshot_cache.set(core.snapshot());

    // The actor may block in `add_child` until the main thread runs the build, so the main
    // thread (this function, window events, commands) only ever sends to it.
    observe_resume(&host, feed.clone());
    let env = ActorEnv {
        host,
        store,
        cache: snapshot_cache.clone(),
        read_only,
    };
    spawn_actor(core, inputs, env)?;
    spawn_ticker(feed.clone())?;
    watch_main_window(&window, feed.clone(), snapshot_cache.clone());
    feed(Input::Started {
        width: size.width,
        height: size.height,
    });
    Ok(())
}

/// A Tauri builder with the plugins. The single-instance guard comes first (see
/// `host_tauri::guard_single_instance`); a development instance with its own config folder runs
/// beside the regular one.
fn builder_with_plugins(dev: &DevHooks) -> tauri::Builder<Wry> {
    let builder = tauri::Builder::default();
    let builder = if dev.config_dir.is_some() {
        builder
    } else {
        host_tauri::guard_single_instance(builder, MAIN_WINDOW)
    };
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
}

fn main() {
    host_tauri::clean_launch_environment();
    // Started as the update helper (see `updater::helper`): swap the bundle, never open a window.
    if let Some(exit_code) = updater::helper::run_if_requested() {
        std::process::exit(exit_code);
    }
    let dev = DevHooks::from_env();
    let context = tauri::generate_context!();
    host_tauri::set_app_id(&context.config().identifier);
    let (sender, inputs) = mpsc::channel::<Input>();
    let feed: Feed = Arc::new(move |input| {
        // Fails only after the actor is gone (app shutting down).
        let _ = sender.send(input);
    });
    let snapshot_cache = SnapshotCache::default();
    let (menu_feed, setup_feed, setup_cache) = (feed.clone(), feed.clone(), snapshot_cache.clone());
    let crash_feed = feed.clone();

    let builder = builder_with_plugins(&dev)
        .manage(Shared {
            feed,
            snapshot: snapshot_cache,
        })
        .invoke_handler(with_commands!(handler))
        // App menu and tray menu items (built by the host from the core's menu model).
        .on_menu_event(move |app, event| {
            host_tauri::handle_menu_event(app, MAIN_WINDOW, event.id().as_ref(), &menu_feed);
        });
    // A view whose web content process died is reloaded through the core.
    let builder = host_tauri::register_crash_hook(builder, &crash_feed);
    let app = builder
        .setup(move |app| Ok(setup(app, &dev, &setup_feed, inputs, &setup_cache)?))
        .build(context)
        .expect("error while building Atrium");
    app.run(|app, event| match event {
        // Dock icon click: bring a hidden (closed to background / started hidden) window back.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => main_window::show_main_window(app),
        // "Restart/Quit and update": the helper swaps the bundle once this process is gone.
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

    /// Tauri binds a command by the name of its function, the UI calls it by the protocol
    /// constant: renaming one without the other must fail here, not at runtime.
    #[test]
    fn commands_are_named_like_their_protocol_constants() {
        macro_rules! names {
            ($($($segment:ident)::+ => $constant:ident),* $(,)?) => {
                [$((stringify!($($segment)::+), protocol::$constant)),*]
            };
        }
        let commands = with_commands!(names);
        for (path, constant) in commands {
            assert_eq!(path.rsplit("::").next().unwrap().trim(), constant);
        }
    }

    /// Service webviews are children of the main window, so a window pattern in the UI
    /// capability would grant them IPC as soon as only their origin stood in the way.
    #[test]
    fn ui_capability_targets_only_the_ui_webview() {
        let capability: Value = serde_json::from_str(include_str!("../capabilities/ui.json"))
            .expect("capability is valid JSON");
        assert!(capability.get("windows").is_none());
        assert_eq!(
            capability["webviews"],
            serde_json::json!([protocol::UI_WEBVIEW])
        );
    }

    /// Only the dialog windows may answer dialogs; they get nothing else.
    #[test]
    fn script_dialog_capability_targets_only_the_dialog_windows() {
        let capability: Value =
            serde_json::from_str(include_str!("../capabilities/script-dialog.json"))
                .expect("capability is valid JSON");
        assert!(capability.get("webviews").is_none());
        assert_eq!(
            capability["windows"],
            serde_json::json!(["script-dialog-*"])
        );
        assert_eq!(
            capability["permissions"],
            serde_json::json!(["script-dialog:default"])
        );
    }
}
