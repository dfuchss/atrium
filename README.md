<p align="center">
  <img src="app/src-tauri/icons/src/atrium.svg" width="128" alt="Atrium logo">
</p>

<h1 align="center">Atrium</h1>

<p align="center"><em>All your messengers in one calm place.</em></p>

<p align="center">
  <img src="docs/screenshots/main.png" alt="Atrium with a Matrix chat open and several accounts in the sidebar" width="800">
</p>

Atrium is a lightweight messenger aggregator for macOS and Linux. It puts Element, Slack, Discord, WhatsApp, Teams and any other web app into one window with a compact sidebar. Every account runs in its own isolated web view, so you can stay signed in to several accounts of the same service side by side.

It is written in Rust on top of [Tauri v2](https://v2.tauri.app/) and uses the system's WebKit (WebKitGTK on Linux) instead of bundling Chromium. Release builds exist for macOS only; on Linux, build from source (see [Linux](DEVELOPMENT.md#linux) in DEVELOPMENT.md).

## Features

- **One window, many accounts.** A sidebar of service icons with unread badges.
  - Reorder accounts with drag and drop.
  - Pick small, medium or large icons.
- **Isolated sessions.** Each account has its own persistent WebKit data store (cookies, storage, caches), e.g. two Slack workspaces or a work and a private Element account.
- **Notifications.**
  - Native notifications; clicking one jumps to the right account.
  - Dock badge and menu bar icon with the total unread count (launcher badge and tray icon on Linux).
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
- **Desktop integration.**
  - App menu with shortcuts (macOS).
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

The screenshots use a local demo Matrix server with made-up users (see [DEVELOPMENT.md](DEVELOPMENT.md#screenshots)).

## Installation

On macOS, download `atrium-macos-arm64.zip` (Apple silicon) or `atrium-macos-x86_64.zip` (Intel) from the [latest release](https://github.com/dfuchss/atrium/releases/latest), unzip it and move `Atrium.app` to the Applications folder.

The builds are not signed, so macOS blocks the first start: open Atrium once, then allow it in System Settings → Privacy & Security → *Open Anyway*. If macOS reports the app as "damaged", remove the download quarantine with `xattr -cr /Applications/Atrium.app`.

### Updates

Atrium checks for a new release on startup and every six hours (Settings → About; the check can be switched off there). An update is downloaded, verified against the release's `SHA256SUMS` and placed next to the app; *Restart and update* quits Atrium, swaps in the new version and starts it again. This works only for an Atrium in `/Applications` or `~/Applications`; development builds and other locations just point to the release page.

Linux has no release builds and no self-update yet; see [Linux](#linux).

To build Atrium yourself, see [Building from source](DEVELOPMENT.md#building-from-source) in [DEVELOPMENT.md](DEVELOPMENT.md).

### Linux

Atrium runs on Linux with WebKitGTK 4.1 when built from source; how to build it is described under [Linux](DEVELOPMENT.md#linux) in DEVELOPMENT.md.

Data locations on Linux:
- **Config:** `~/.config/org.fuchss.atrium/` (`accounts.json`, `services/`)
- **Web data (per account):** `~/.local/share/org.fuchss.atrium/webdata/<account id>/`. WebKitGTK keeps the cookies there as plain text, and its content processes run without WebKit's bubblewrap sandbox, so a page that breaks out of one could read the web data of every account.
- **Log:** the app log folder, `~/.local/share/org.fuchss.atrium/logs/atrium.log`

Known limitations on Linux:
- JavaScript dialogs are WebKitGTK's own: no per-account title, suppression or rate limit.
- Sign-in popups open like other links (by default in the system browser): on Linux the new-window request does not carry the size a page asks for.
- There is no app menu, so no keyboard shortcuts for settings or quit; use the tray menu.
- The unread count appears as the tray label only where the panel supports it. The launcher badge works only where the desktop supports the Unity launcher API.
- "Open links in" takes a command name (e.g. `firefox`) instead of an application name.
- No self-update.
- On Wayland, starting Atrium again may only highlight the running Atrium in the task bar instead of bringing its window to the front.
- Session cookies are stored unencrypted in the per-account web data folder (mode `0700`): exclude `webdata/` from backups and sync.

## Usage

- **Add accounts** with the **+** button at the bottom of the sidebar. The settings dialog stays open, so you can add several in a row. Drag icons to reorder them.
- **Settings** (gear button or <kbd>⌘</kbd><kbd>,</kbd>):
  - Accounts
  - Custom services
  - General
  - Notifications
  - Dock badge (Launcher badge on Linux)
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

- **Isolation:** every service view runs in its own WebKit content process (sandboxed on macOS, see [Linux](#linux)) with its own data store. Service pages get **no access** to Atrium's internal API.
- **Minimal script injection:** Atrium injects only a small script that forwards web notifications and routes new-window links, plus optional custom CSS. Unread counts are read from the page title in Rust.
- **Links:** only `http(s)` and `mailto` links are handed to the system, and in-page navigation is limited to web schemes.
- **Local only:** the settings UI runs under a strict Content Security Policy. Atrium has no telemetry and no server; everything stays on your computer. The update check asks the GitHub API for the latest release (with the Atrium version in the user agent); it can be switched off.

## Known limitations

- **Passkeys/WebAuthn don't work** inside embedded WebKit views for third-party sites. Apple restricts this to apps with a special browser entitlement. Use password + TOTP or SSO instead.
- **Some sites don't render in WebKit at all.** If a service stays blank in Safari too, Atrium can't show it either. A Chromium-based engine is on the roadmap.
- **Sleeping services** don't deliver notifications until they are opened again.

## Data locations

- **Config (macOS):** `~/Library/Application Support/org.fuchss.atrium/`
- **Log (macOS):** `~/Library/Logs/org.fuchss.atrium/atrium.log` (readable by you only, rotated to `atrium.log.1` at 5 MB). URLs are logged without query and fragment, page titles not at all.
- **Web data (macOS, per account):** `~/Library/WebKit/org.fuchss.atrium/`
- Linux: see [Linux](#linux).

## Development

Building from source, the architecture, checks, debug options and releasing: see [DEVELOPMENT.md](DEVELOPMENT.md).

## License

Atrium is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version. See [`LICENSE`](LICENSE).

Copyright © 2026 Dominik Fuchß
