# Messengers

A messenger aggregator in Rust (Tauri v2). It shows one window with a sidebar. Each service account runs in its own isolated webview with its own data store, so cookies and storage stay separate per account.

## Layout

| Path | What |
|---|---|
| `crates/domain` | Pure core: service catalog, reducer `Core::handle(Input) -> Vec<Effect>`, port traits, effect runner. No Tauri, no IO. |
| `crates/protocol` | Types shared by the backend and the UI (`UiCommand`, `Snapshot`). Compiles to wasm. |
| `crates/host-tauri` | Adapter: implements the ports on Tauri multiwebview (webviews, notifications, dock badge, opener). |
| `crates/store-fs` | Adapter: atomic JSON store (`accounts.json`). |
| `app/src-tauri` | Composition root: window, actor thread, Tauri commands. |
| `app/ui` | Sidebar and manage dialog in Leptos (CSR, Trunk). |
| `services/*.toml` | Built-in service definitions. User overrides go in `<config>/services/*.toml`. |
| `spikes/multiwebview` | Phase-0 spike, throwaway (see `docs/spikes.md`). |

The only JavaScript injected into service pages is `crates/domain/assets/notify-shim.js`. It queues web notifications, and Rust drains that queue by polling. Service pages get no Tauri IPC capability.
The other injected scripts are an optional per-account `<style>` injector (custom CSS) and, in popup windows, a `window.close` hook. New windows never get `NewWindowResponse::Allow`. A `window.open` with an explicit size is a sign-in popup and always opens as an in-app window. Every other new window follows the link target: the system browser or an in-app window (see `crates/host-tauri/src/links.rs`).

## Build and run (macOS 14+)

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"   # rustup from Homebrew is keg-only
cargo test --workspace                              # domain + store-fs tests
cd app/src-tauri && cargo tauri dev                 # dev: trunk serve + app
cd app/src-tauri && cargo tauri build --debug       # bundle: target/debug/bundle/macos/Messengers.app
```

Use the bundled `.app` to test notifications and the dock badge.

Where things are stored:
- **Config:** `~/Library/Application Support/org.fuchss.messengers/accounts.json`
- **Log:** `~/Library/Logs/org.fuchss.messengers/messengers.log`
- **Web data, per account:** `~/Library/WebKit/org.fuchss.messengers`
