<p align="center">
  <img src="app/src-tauri/icons/src/atrium.svg" width="128" alt="Atrium logo">
</p>

<h1 align="center">Atrium</h1>

<p align="center"><em>All your messengers in one calm place.</em></p>

<p align="center">
  <img src="docs/screenshots/main.png" alt="Atrium with a Matrix chat open and several accounts in the sidebar" width="800">
</p>

Atrium is a lightweight messenger aggregator for macOS. It puts Element, Slack, Discord, WhatsApp, Teams and any other web app into one window with a compact sidebar. Every account runs in its own isolated web view, so you can stay signed in to several accounts of the same service side by side.

It is written in Rust on top of [Tauri v2](https://v2.tauri.app/) and uses the system's WebKit instead of bundling Chromium.

## Features

- **One window, many accounts.** A sidebar of service icons with unread badges.
  - Reorder accounts with drag and drop.
  - Pick small, medium or large icons.
- **Isolated sessions.** Each account has its own persistent WebKit data store (cookies, storage, caches), e.g. two Slack workspaces or a work and a private Element account.
- **Notifications.**
  - Native macOS notifications; clicking one jumps to the right account.
  - Dock badge and menu bar icon with the total unread count.
  - Do Not Disturb, and per-account mute.
  - Optional hiding of message text.
- **Sleep.**
  - Inactive services can be put to sleep automatically, globally or per account, to free memory.
  - Sleeping accounts are shown greyed out with a moon.
  - An account can start asleep, so it only loads when you select it.
- **Links.**
  - Links open in your browser of choice, or in an app window that shares the account's login.
  - Sign-in popups (SSO/OAuth) always stay in the app.
  - Per account, you can list hosts that always open in the app, e.g. your single-sign-on provider.
- **Icons.** Site favicons are fetched automatically. You can also use a [Simple Icons](https://simpleicons.org/) glyph or your own image.
- **Customisable.**
  - Per account: zoom, user agent, custom CSS, camera and microphone permission, enable/disable, force reload without the cache.
  - Custom services can be created in the settings, optionally starting from a built-in one.
  - **Expert mode:** edit service definitions as TOML files in the services folder and reload them without restarting.
- **macOS integration.**
  - App menu with shortcuts.
  - Launch at login, start hidden, keep running in the background.
  - After you wake your Mac from sleep, reload only the services that stopped responding, or all of them, per account (not on Power Nap background wakes).

## Built-in services

| Service | Notes |
|---|---|
| Element / Matrix | Any Element web host (default `app.element.io`). |
| Slack | |
| Microsoft Teams | |
| Discord | |
| WhatsApp | |
| Rainbow | |
| LinkedIn | Messaging |
| Mastodon | Any instance |
| Bluesky | |

Anything else can be added as a custom service: a name, a URL, and optionally a title pattern for unread counts.

## Screenshots

| Account settings | General settings |
|---|---|
| ![Account settings](docs/screenshots/settings-accounts.png) | ![General settings](docs/screenshots/settings-general.png) |
| **Custom services** | |
| ![Custom services and service definition files](docs/screenshots/custom-services.png) | |

The screenshots use a local demo Matrix server with made-up users (see [Development](#development)).

## Installation

Download `atrium-macos-arm64.zip` (Apple silicon) or `atrium-macos-x86_64.zip` (Intel) from the [latest release](https://github.com/dfuchss/atrium/releases/latest), unzip it and move `Atrium.app` to the Applications folder.

The builds are not signed, so macOS blocks the first start: open Atrium once, then allow it in System Settings → Privacy & Security → *Open Anyway*. If macOS reports the app as "damaged", remove the download quarantine with `xattr -cr /Applications/Atrium.app`.

### Updates

Atrium checks for a new release on startup and every six hours (Settings → About; the check can be switched off there). An update is downloaded, verified against the release's `SHA256SUMS` and placed next to the app; *Restart and update* quits Atrium, swaps in the new version and starts it again. This works only for an Atrium in `/Applications` or `~/Applications`; development builds and other locations just point to the release page.

Releases are built by `.github/workflows/release.yml` when a tag `v<version>` is pushed; the tag must match the version in the workspace `Cargo.toml`, the only place the version is set.

### Building from source

Requirements:
- macOS 14 or later. Per-account data stores need WebKit's `WKWebsiteDataStore(forIdentifier:)`.
- [Rust](https://rustup.rs/) (stable) with the `wasm32-unknown-unknown` target.
- [Trunk](https://trunkrs.dev/) and the [Tauri CLI](https://v2.tauri.app/reference/cli/).

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk tauri-cli --locked   # or: brew install trunk

git clone https://github.com/dfuchss/atrium.git
cd atrium/app/src-tauri
cargo tauri build                        # → target/release/bundle/macos/Atrium.app
```

Copy `Atrium.app` to `/Applications`. The app is not signed; the first time, open it as described above.

## Usage

- **Add accounts** with the **+** button at the bottom of the sidebar. The settings dialog stays open, so you can add several in a row. Drag icons to reorder them.
- **Settings** (gear button or <kbd>⌘</kbd><kbd>,</kbd>):
  - Accounts
  - Custom services
  - General
  - Notifications
  - Dock badge
  - Links
  - About

| Shortcut | Action |
|---|---|
| <kbd>⌘</kbd><kbd>1</kbd> … <kbd>⌘</kbd><kbd>9</kbd> | Switch to account 1–9 |
| <kbd>⌘</kbd><kbd>R</kbd> | Reload the current account |
| <kbd>⌘</kbd><kbd>[</kbd> / <kbd>⌘</kbd><kbd>]</kbd> | Back / forward |
| <kbd>⌘</kbd><kbd>=</kbd> / <kbd>⌘</kbd><kbd>-</kbd> / <kbd>⌘</kbd><kbd>0</kbd> | Zoom in / out / reset (per account) |
| <kbd>⌘</kbd><kbd>⇧</kbd><kbd>O</kbd> | Open the current page in the browser |
| <kbd>⌘</kbd><kbd>W</kbd> | Close the window (Atrium keeps running in the background) |
| <kbd>⌘</kbd><kbd>Q</kbd> | Quit |

### Service definitions (expert mode)

Settings → Custom services → *Open folder* opens `~/Library/Application Support/org.fuchss.atrium/services/`. Every `*.toml` file there defines a service, in the same format as the built-in ones in [`services/`](services). A file with the id of a built-in service overrides it. The same tab lists the files and edits them in the app: *Edit*, *New definition* (starts from the documented template), *Disable*/*Enable* (renames to `.toml.disabled`) and *Delete*. The editor checks the text with the loader's own rules while you type, shows errors with line and column, refuses to save an invalid file and applies a saved file right away. Edits made outside the app need *Reload*.

```toml
id = "forum"
name = "Team Forum"
color = "#0EA5E9"
url = "https://{instance}/"             # {instance} is asked for per account
instance_label = "Forum host"
user_agent = "webkit"               # webkit | chrome | default | any literal UA
internal_hosts = ["{instance}"]

[unread]
title_regex = '^\((\d+)\)'          # capture group 1 = unread count
```

`{instance}` stands for the part of the URL that differs per account, e.g. the server host of a self-hosted service or a workspace subdomain. Each account of the service asks for it (labelled with `instance_label`, pre-filled with `instance_default`), and it can be used in `url` and `internal_hosts`.

## Privacy and security

- **Isolation:** every service view runs in its own sandboxed WebKit content process with its own data store. Service pages get **no access** to Atrium's internal API.
- **Minimal script injection:** Atrium injects only a small script that forwards web notifications and routes new-window links, plus optional custom CSS. Unread counts are read from the page title in Rust.
- **Links:** only `http(s)` and `mailto` links are handed to macOS, and in-page navigation is limited to web schemes.
- **Local only:** the settings UI runs under a strict Content Security Policy. Atrium has no telemetry and no server; everything stays on your Mac. The update check asks the GitHub API for the latest release (with the Atrium version in the user agent); it can be switched off.

## Known limitations

- **Passkeys/WebAuthn don't work** inside embedded WebKit views for third-party sites. Apple restricts this to apps with a special browser entitlement. Use password + TOTP or SSO instead.
- **Some sites don't render in WebKit at all.** If a service stays blank in Safari too, Atrium can't show it either. A Chromium-based engine is on the roadmap.
- **Sleeping services** don't deliver notifications until they are opened again.

## Architecture

Atrium follows a hexagonal design. A pure Rust core decides; adapters execute.

| Path | What |
|---|---|
| `crates/domain` | Pure core: service catalog, reducer `Core::handle(Input) -> Vec<Effect>`, port traits, effect runner. No Tauri, no IO. |
| `crates/protocol` | Types shared by the backend and the UI (`UiCommand`, `Snapshot`). Compiles to wasm. |
| `crates/host-tauri` | Adapter: implements the ports on Tauri multi-webview (webviews, notifications, dock badge, tray, menus, links). |
| `crates/store-fs` | Adapter: atomic JSON store (`accounts.json`). |
| `crates/updater` | Self-update from GitHub Releases: check, verify, stage next to the app, swap after quitting. |
| `crates/webview-dialogs` | Adapter: native `alert`/`confirm`/`prompt` panels for the service pages, with platform-neutral dialog types; on macOS added to the system webview's delegate at runtime. The only crate with `unsafe` code. |
| `app/src-tauri` | Composition root: window, actor thread, Tauri commands. |
| `app/ui` | Sidebar and settings in [Leptos](https://leptos.dev/) (CSR, built with Trunk). |
| `services/*.toml` | Built-in service definitions. |

The web engine sits behind the `WebviewHost` port, so a Chromium (CEF) backend can be added later without touching the core.

Data locations:
- **Config:** `~/Library/Application Support/org.fuchss.atrium/`
- **Log:** `~/Library/Logs/org.fuchss.atrium/atrium.log` (readable by you only, rotated to `atrium.log.1` at 5 MB). URLs are logged without query and fragment, page titles not at all.
- **Web data (per account):** `~/Library/WebKit/org.fuchss.atrium/`

## Development

```sh
scripts/check.sh                               # everything CI checks: fmt, UI build, clippy, tests
cargo test --workspace --exclude atrium-ui     # core, adapters, store
cd app/src-tauri && cargo tauri dev            # run with live-reloading UI
cd app/src-tauri && cargo tauri build --debug  # bundle: target/debug/bundle/macos/Atrium.app
```

`scripts/check.sh` must pass before every commit; the GitHub workflow runs it too. The lint rules are in the `[workspace.lints]` section of `Cargo.toml` (clippy pedantic plus a few denials, each allowed exception with its reason), the thresholds for function length, complexity and arguments in `clippy.toml`.

### Releasing

Notes for the user-visible changes go under `## [Unreleased]` in [`CHANGELOG.md`](CHANGELOG.md) as they happen. To release:
1. Rename the Unreleased content to a `## X.Y.Z` section and leave a fresh empty `## [Unreleased]` above it.
2. Set the version in the workspace `Cargo.toml`.
3. Commit and tag `vX.Y.Z`.

The release workflow publishes that section plus the install text from `.github/release-install.md` (built by `.github/scripts/release-notes.sh`) and fails if the section is missing or empty.

Debug builds (`cargo tauri dev`, `cargo tauri build --debug`) read these environment variables; release builds ignore them. They let a second, isolated instance run next to the regular one, e.g. for screenshots:

| Variable | Effect |
|---|---|
| `ATRIUM_CONFIG_DIR=<dir>` | Config folder instead of the app's: `accounts.json`, `services/` and `atrium.log`. Such an instance never changes the login item. |
| `ATRIUM_ALWAYS_ON_TOP=1` | Keeps the main window on top. macOS does not paint occluded windows, so `screencapture -l` would capture a blank window. |
| `ATRIUM_WINDOW_POSITION=<x>,<y>` | Position of the main window in logical pixels (the main display starts at `0,0`), e.g. to open it on a Retina display. |
| `ATRIUM_DEV_INIT_SCRIPT=<file>` | JavaScript appended to the init script of every service view (the screenshot script seeds a demo Element session with it). |
| `ATRIUM_LOG_TITLES=1` | Logs page titles with each unread change, to check a service's `title_regex` against what it really shows. Titles name chats and senders, so this is off otherwise. |
| `ATRIUM_DEV_UI_QUERY=<query>` | Query string for the sidebar UI. `dialog=<tab>` opens the settings dialog on a tab (`accounts`, `general`, `notifications`, `dock`, `links`, `custom`, `about`), `edit=<index>` opens the edit form of an account, `addcustom=1` the custom service form, `editservice=<file>` the definition editor (demo files `wiki.toml`, `broken.toml`). |

`scripts/screenshots.sh` takes the README screenshots in `docs/screenshots/` with demo data only. It runs a throwaway Matrix homeserver (tuwunel) and Element web in Docker, seeds demo users and chats, builds a debug bundle with its own bundle id `org.fuchss.atrium.demo` into `target/screenshots`, and starts it with the variables above and a temporary config folder. The own bundle id keeps the demo's WebKit data, logs, caches and WebCrypto Keychain item apart from a regular installation (sharing `org.fuchss.atrium` would make macOS ask for access to its Keychain item). At the end it removes the containers, the temporary folder and everything stored under `org.fuchss.atrium.demo`, including its Keychain item. It needs Docker, ImageMagick (`magick`), free ports 8008 and 8080, and Screen Recording permission for the terminal.


## License

Atrium is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version. See [`LICENSE`](LICENSE).

Copyright © 2026 Dominik Fuchß
