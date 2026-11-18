# Architecture

Atrium follows a hexagonal design. A pure Rust core decides; adapters execute.

| Path | What |
|---|---|
| `crates/domain` | Pure core: service catalog, reducer `Core::handle(Input) -> Vec<Effect>`, port traits, effect runner. No Tauri, no IO. |
| `crates/protocol` | Types shared by the backend and the UI (`UiCommand`, `Snapshot`). Compiles to wasm. |
| `crates/host-tauri` | Adapter: implements the ports on Tauri multi-webview (webviews, notifications, dock badge, tray, menus, links); platform-specific code in `native/`. |
| `crates/store-fs` | Adapter: atomic JSON store (`accounts.json`). |
| `crates/fs-util` | File operations shared by the crates: atomic writes with a fixed mode; the poison-tolerant mutex lock. |
| `crates/updater` | Self-update from GitHub Releases: check, verify, stage next to the app, swap after quitting; the macOS app bundle, the Linux AppImage or the Windows executable, platform-specific code in `native/`. |
| `crates/platform-ffi` | Adapter: the engine and OS calls Tauri has no API for. Native `alert`/`confirm`/`prompt` panels for the service pages, with platform-neutral dialog types; on macOS added to the system webview's delegate at runtime, on Linux shown from WebKitGTK's `script-dialog` signal, on Windows taken from WebView2's `ScriptDialogOpening` event and shown in a window of Atrium's UI; on macOS also the other WebKit calls (service view configuration, reloads from origin, display wakes), on Windows the other WebView2 and Win32 calls and the Start menu shortcuts. The only crate built on `unsafe` code; elsewhere it is denied, except for one environment change at start-up on Linux in `host-tauri`. |
| `app/src-tauri` | Composition root: window, actor thread, Tauri commands. |
| `app/ui` | Sidebar and settings in [Leptos](https://leptos.dev/) (CSR, built with Trunk). |
| `services/*.toml` | Built-in service definitions (see [Built-in services](Services)). |

The web engine sits behind the `WebviewHost` port, so a Chromium (CEF) backend can be added later without touching the core.
