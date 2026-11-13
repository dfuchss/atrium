# Atrium

Atrium is a lightweight messenger aggregator for macOS, built with Rust and Tauri v2. It shows one window with a sidebar. Each service account runs in its own isolated webview with its own data store, so cookies and storage stay separate per account.

Features:
- One window with a sidebar of service accounts (Slack, Discord, WhatsApp, Teams, Element, Bluesky, Mastodon and more, plus custom sites).
- Per-account isolated data stores, so several logins per service work side by side.
- Native notifications, dock badge, menu bar tray icon and Do Not Disturb.
- Configurable link handling, per-account custom CSS, and user service definitions in TOML.

Atrium is free software under the GNU General Public License v3.0 or later (GPL-3.0-or-later). Copyright (C) 2026 Dominik Fuchß. See `LICENSE`.

Project page: https://github.com/dfuchss/atrium

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
cargo test --workspace --exclude atrium-ui                            # domain + store-fs tests
cd app/src-tauri && cargo tauri dev                 # dev: trunk serve + app
cd app/src-tauri && cargo tauri build --debug       # bundle: target/debug/bundle/macos/Atrium.app
```

Use the bundled `.app` to test notifications and the dock badge.

Where things are stored:
- **Config:** `~/Library/Application Support/org.fuchss.atrium/accounts.json`
- **Log:** `~/Library/Logs/org.fuchss.atrium/atrium.log`
- **Web data, per account:** `~/Library/WebKit/org.fuchss.atrium`


