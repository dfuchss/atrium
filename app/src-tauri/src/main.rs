//! Composition root: builds the core, the Tauri adapter and the file store, and runs the
//! actor thread that owns [`domain::Core`]. Every input (UI commands, native hooks, window
//! events, ticks) goes through one channel; effects are applied in order by the actor.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
    AppHandle, LogicalPosition, LogicalSize, Manager, RunEvent, State, Theme, TitleBarStyle,
    WebviewBuilder, WebviewUrl, WindowEvent, Wry,
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
    state
        .snapshot
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn load_persisted(store: &dyn Store, log: &dyn Platform) -> Persisted {
    store.load().unwrap_or_else(|e| {
        log.log(&format!("cannot load accounts, starting empty: {e}"));
        Persisted::default()
    })
}

/// Wall clock in ms since the Unix epoch (ticks; a jump > 30 s means the Mac slept).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Owns the core; never returns while the channel is open.
fn run_actor(
    mut core: Core,
    inputs: Receiver<Input>,
    host: TauriHost<Wry>,
    store: FsStore,
    cache: Arc<Mutex<Snapshot>>,
) {
    let ports = Ports {
        host: &host,
        notifier: &host,
        platform: &host,
        ui: &host,
        store: &store,
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
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(Shared {
            feed: feed.clone(),
            snapshot: cache.clone(),
        })
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
            let services_dir = config_dir.join("services");
            let (user_defs, user_errors) = host_tauri::services::read_user_services(&services_dir);
            host.log(&format!(
                "config {}, {} accounts, {} user services, {} service errors",
                config_dir.display(),
                persisted.accounts.len(),
                user_defs.len(),
                user_errors.len()
            ));

            // Transparent title bar without a title: it shows the window background (the
            // sidebar color) instead of a lighter strip. Not Overlay: the content view stays
            // below the title bar, so the services' top-edge controls keep working.
            let window = WindowBuilder::new(app, MAIN_WINDOW)
                .title("Messengers")
                .title_bar_style(TitleBarStyle::Transparent)
                .hidden_title(true)
                .inner_size(1280.0, 800.0)
                .min_inner_size(600.0, 400.0)
                .visible(!persisted.settings.app.start_hidden)
                .build()?;
            window.set_background_color(Some(window_background(
                window.theme().unwrap_or(Theme::Light),
            )))?;
            let size = window
                .inner_size()?
                .to_logical::<f64>(window.scale_factor()?);
            window.add_child(
                WebviewBuilder::new(UI_WEBVIEW, WebviewUrl::App("index.html".into())),
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(
                    persisted.settings.app.icon_size.sidebar_width(),
                    size.height,
                ),
            )?;

            let mut core = Core::new(Catalog::builtin(), persisted, Box::new(uuid::Uuid::new_v4));
            core.set_user_services(user_defs, user_errors);
            core.set_app_info(AppInfo {
                version,
                config_dir: config_dir.display().to_string(),
                services_dir: services_dir.display().to_string(),
                log_file: host
                    .log_file()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            });
            *cache.lock().unwrap_or_else(PoisonError::into_inner) = core.snapshot();
            let settings_cache = cache.clone();

            // The actor may block in `add_child` until the main thread runs the build, so the
            // main thread (this closure, window events, commands) only ever sends to it.
            std::thread::Builder::new()
                .name("core".into())
                .spawn(move || run_actor(core, rx, host, store, cache))?;
            let tick = feed.clone();
            std::thread::Builder::new()
                .name("ticker".into())
                .spawn(move || {
                    loop {
                        std::thread::sleep(Duration::from_secs(1));
                        tick(Input::Tick { now_ms: now_ms() });
                    }
                })?;

            let (resize, event_window) = (feed.clone(), window.clone());
            // Registered on the main window only: popup and app windows opened by services
            // are normal windows that really close.
            window.on_window_event(move |event| match event {
                // Closing the window (red button, Cmd+W) hides it or quits, as the settings say;
                // Quit (Cmd+Q) is a menu item (see `host_tauri::graceful_quit`).
                WindowEvent::CloseRequested { api, .. } => {
                    let to_background = settings_cache
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .settings
                        .close_to_background;
                    api.prevent_close();
                    if to_background {
                        let _ = event_window.hide();
                    } else {
                        // Keep the window (and its service views) until the pages are unloaded.
                        host_tauri::graceful_quit(event_window.app_handle());
                    }
                }
                WindowEvent::ThemeChanged(theme) => {
                    let _ = event_window.set_background_color(Some(window_background(*theme)));
                }
                WindowEvent::Resized(size) => {
                    // A Resized event reports the new physical size; the scale factor is
                    // unchanged (ScaleFactorChanged carries its own).
                    if let Ok(scale) = event_window.scale_factor() {
                        let size = size.to_logical::<f64>(scale);
                        resize(Input::WindowResized {
                            width: size.width,
                            height: size.height,
                        });
                    }
                }
                WindowEvent::ScaleFactorChanged {
                    scale_factor,
                    new_inner_size,
                    ..
                } => {
                    let size = new_inner_size.to_logical::<f64>(*scale_factor);
                    resize(Input::WindowResized {
                        width: size.width,
                        height: size.height,
                    });
                }
                _ => {}
            });

            feed(Input::Started {
                width: size.width,
                height: size.height,
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building messengers");
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
