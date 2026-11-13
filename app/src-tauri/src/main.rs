//! Composition root: builds the core, the Tauri adapter and the file store, and runs the
//! actor thread that owns [`domain::Core`]. Every input (UI commands, native hooks, window
//! events, ticks) goes through one channel; effects are applied in order by the actor.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use domain::ports::{Platform, Store};
use domain::runtime::{Ports, apply};
use domain::{Catalog, Core, Effect, Input, Persisted};
use host_tauri::{Feed, TauriHost};
use protocol::{AppInfo, SIDEBAR_WIDTH, Snapshot, UI_WEBVIEW, UiCommand};
use store_fs::FsStore;
use tauri::window::WindowBuilder;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, RunEvent, State, WebviewBuilder, WebviewUrl, WindowEvent, Wry,
};
use tauri_plugin_autostart::MacosLauncher;

const MAIN_WINDOW: &str = "main";

/// State shared with the Tauri commands.
struct Shared {
    feed: Feed,
    /// Last snapshot emitted by the core; served to the UI on (re)load.
    snapshot: Arc<Mutex<Snapshot>>,
}

#[tauri::command]
fn ui_command(state: State<'_, Shared>, cmd: UiCommand) {
    (state.feed)(Input::Ui(cmd));
}

#[tauri::command]
fn get_snapshot(state: State<'_, Shared>) -> Snapshot {
    state.snapshot.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn load_persisted(store: &dyn Store, log: &dyn Platform) -> Persisted {
    store.load().unwrap_or_else(|e| {
        log.log(&format!("cannot load accounts, starting empty: {e}"));
        Persisted::default()
    })
}

/// Wall clock in ms since the Unix epoch (ticks; a jump > 30 s means the Mac slept).
fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Built-in services plus user definitions from `<config>/services/*.toml` (overriding by id).
/// The core adds the custom services stored in `accounts.json` on top.
fn load_catalog(config_dir: &Path, log: &dyn Platform) -> Catalog {
    let mut catalog = Catalog::builtin();
    let Ok(entries) = std::fs::read_dir(config_dir.join("services")) else { return catalog };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    for path in paths {
        let source = path.display().to_string();
        match std::fs::read_to_string(&path) {
            Ok(text) => match Catalog::parse(&source, &text) {
                Ok(def) => {
                    log.log(&format!("loaded service {:?} from {source}", def.id));
                    catalog.insert(def);
                }
                Err(e) => log.log(&format!("invalid service definition: {e}")),
            },
            Err(e) => log.log(&format!("cannot read {source}: {e}")),
        }
    }
    catalog
}

/// Owns the core; never returns while the channel is open.
fn run_actor(
    mut core: Core,
    inputs: Receiver<Input>,
    host: TauriHost<Wry>,
    store: FsStore,
    cache: Arc<Mutex<Snapshot>>,
) {
    let ports = Ports { host: &host, notifier: &host, platform: &host, ui: &host, store: &store };
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
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn main() {
    let (tx, rx) = mpsc::channel::<Input>();
    let feed: Feed = Arc::new(move |input| {
        // Fails only after the actor is gone (app shutting down).
        let _ = tx.send(input);
    });
    let cache = Arc::new(Mutex::new(Snapshot::default()));
    let menu_feed = feed.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .manage(Shared { feed: feed.clone(), snapshot: cache.clone() })
        .invoke_handler(tauri::generate_handler![ui_command, get_snapshot])
        // App menu and tray menu items (built by the host from the core's menu model).
        .on_menu_event(move |app, event| {
            host_tauri::handle_menu_event(app, MAIN_WINDOW, event.id().as_ref(), &menu_feed);
        })
        .setup(move |app| {
            // The host only needs the window lazily, so it can log while loading the settings
            // that decide how the window is created.
            let host = TauriHost::new(app.handle().clone(), MAIN_WINDOW, feed.clone());
            let version = app.package_info().version.to_string();
            host.log(&format!("---- messengers {version} starting ----"));
            let config_dir = app.path().app_config_dir()?;
            let store = FsStore::new(config_dir.clone());
            let persisted = load_persisted(&store, &host);
            let catalog = load_catalog(&config_dir, &host);
            host.log(&format!(
                "config {}, {} accounts, {} services",
                config_dir.display(),
                persisted.accounts.len(),
                catalog.services().len()
            ));

            let window = WindowBuilder::new(app, MAIN_WINDOW)
                .title("Messengers")
                .inner_size(1280.0, 800.0)
                .min_inner_size(600.0, 400.0)
                .visible(!persisted.settings.app.start_hidden)
                .build()?;
            let size = window.inner_size()?.to_logical::<f64>(window.scale_factor()?);
            window.add_child(
                WebviewBuilder::new(UI_WEBVIEW, WebviewUrl::App("index.html".into())),
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(SIDEBAR_WIDTH, size.height),
            )?;

            let mut core = Core::new(catalog, persisted, Box::new(uuid::Uuid::new_v4));
            core.set_app_info(AppInfo {
                version,
                config_dir: config_dir.display().to_string(),
                log_file: host.log_file().map(|p| p.display().to_string()).unwrap_or_default(),
            });
            *cache.lock().unwrap_or_else(PoisonError::into_inner) = core.snapshot();
            let settings_cache = cache.clone();

            // The actor may block in `add_child` until the main thread runs the build, so the
            // main thread (this closure, window events, commands) only ever sends to it.
            std::thread::Builder::new()
                .name("core".into())
                .spawn(move || run_actor(core, rx, host, store, cache))?;
            let tick = feed.clone();
            std::thread::Builder::new().name("ticker".into()).spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    tick(Input::Tick { now_ms: now_ms() });
                }
            })?;

            let (resize, event_window) = (feed.clone(), window.clone());
            // Registered on the main window only: popup and app windows opened by services
            // are normal windows that really close.
            window.on_window_event(move |event| match event {
                // Closing the window (red button, Cmd+W) only hides it when the settings say
                // so; Cmd+Q terminates the app without a CloseRequested.
                WindowEvent::CloseRequested { api, .. } => {
                    let to_background =
                        settings_cache.lock().unwrap_or_else(PoisonError::into_inner).settings.close_to_background;
                    if to_background && event_window.label() == MAIN_WINDOW {
                        api.prevent_close();
                        let _ = event_window.hide();
                    }
                }
                WindowEvent::Resized(size) => {
                    // A Resized event reports the new physical size; the scale factor is
                    // unchanged (ScaleFactorChanged carries its own).
                    if let Ok(scale) = event_window.scale_factor() {
                        let size = size.to_logical::<f64>(scale);
                        resize(Input::WindowResized { width: size.width, height: size.height });
                    }
                }
                WindowEvent::ScaleFactorChanged { scale_factor, new_inner_size, .. } => {
                    let size = new_inner_size.to_logical::<f64>(*scale_factor);
                    resize(Input::WindowResized { width: size.width, height: size.height });
                }
                _ => {}
            });

            feed(Input::Started { width: size.width, height: size.height });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building messengers");
    app.run(|app, event| {
        // Dock icon click: bring a hidden (closed to background / started hidden) window back.
        if let RunEvent::Reopen { .. } = event {
            show_main_window(app);
        }
    });
}
