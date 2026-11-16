# Changelog

All notable changes to Atrium are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow
[Semantic Versioning](https://semver.org/) (`X.Y.Z`); a release is tagged `vX.Y.Z`.

## [Unreleased]

### Added

- Atrium runs on Linux (WebKitGTK 4.1) when built from source; each account keeps its web data in a folder of its own; WebKit's web process sandbox is not available on Linux builds (Settings > About says so)
- On Linux, starting Atrium again brings the running Atrium to the front instead of opening a second one
- On Linux, `alert`, `confirm` and `prompt` of service pages show Atrium's own dialogs, as on macOS: named after the account and site, with suppression and a rate limit; before-unload prompts no longer ask
- On Linux, the keyboard shortcuts of the macOS menu work with Ctrl: settings, reload, back and forward, zoom (also Ctrl++ and Ctrl with the numeric keypad's +, - and 0), open in browser, close, quit and switching accounts with Ctrl+1 to Ctrl+9
- On Linux the settings say tray icon, launcher badge and desktop light or dark setting instead of menu bar, dock badge and macOS

### Changed

- **Breaking:** the `safari` user agent preset is renamed `webkit`: services and accounts that set `user_agent = "safari"` must use `"webkit"`
- On Linux the WebKit and Chrome presets report a Linux platform
- New app icon: an atrium courtyard with arched walls around a lit floor

### Fixed

- A woken account no longer keeps a small yellow remnant of its sleeping moon on the sidebar icon on macOS
- Drop-down lists draw their own arrow, so their text is no longer cut off on Linux
- The *Added* note after adding an account appears beside the *Add* button instead of below it, so the settings no longer jump

## 0.2.0

### Added

- Start asleep in an account's settings keeps a service unloaded when Atrium starts; it loads when you select it
- Force reload in an account's settings reloads the service without its cache, for a page that stays broken after a normal reload

### Changed

- After your Mac wakes from sleep, only services that stop responding are reloaded instead of all of them, and only once you wake the Mac, not during Power Nap. Choose Check, Reload or Do nothing in the settings, also per account

### Fixed

- Downloads never replace an existing file: a taken name gets a number, like `report (1).pdf`

### Security

- Site icons are never downloaded from your own computer or local network, even when a host name points there

## 0.1.0

First release.

### Added

- Choose Auto, Light or Dark appearance in the settings; service pages that support it follow along
- All your messengers in one window with a compact sidebar, unread badges, a dock badge and a menu bar icon
- Every account in its own isolated web view, so several accounts of the same service work side by side
- App menu with keyboard shortcuts, launch at login, start hidden and keep running in the background
- Built-in services for Element / Matrix, Slack, Discord, Rainbow, LinkedIn and Mastodon
- Custom services created in the settings
- Element starts reliably after a restart instead of getting stuck on a stale session lock
- Built-in services for WhatsApp, Microsoft Teams and Bluesky
- Accurate Element unread counts, read from the page with the window title as fallback
- Service definitions as TOML files with an in-app editor that checks your text while you type
- Per-account zoom, user agent, custom CSS, site icon, camera and microphone permission, and enable/disable
- Reorder accounts by drag and drop, choose the icon size, pick a Simple Icons glyph, and let inactive services sleep to free memory
- Open and reload the services folder from the settings; sleeping services show a grayscale icon
- Native notifications that open the right account, with Do Not Disturb and per-account mute
- Notifications are capped per account, so no page can flood you
- Links open in your browser of choice or in an app window that keeps the account's login
- A per-account list of hosts that always open in the app
- Popup and app windows show the site's host in their title
- Mail links open only for the account you are looking at, and external links are rate-limited
- The number of in-app windows per account is capped and rate-limited
- Native `alert`, `confirm` and `prompt` dialogs for service pages, naming the account and site, with suppression and a rate limit
- Camera and microphone are only available to the service's own sites
- A private log: no full URLs or page titles, readable only by you, and rotated
- Downloads are quarantined, hidden files are refused, and you are notified when a download finishes
- Removing an account deletes its stored data, retried at the next start if needed
- Site icons are fetched only over https from the page's or service's own hosts
- Your account list is stored readable by you only
- If your saved accounts can't be read, Atrium starts read-only and leaves them untouched
- Removing an account also closes its popup windows before its data is deleted
- Automatic updates from Settings > About with verified downloads and a one-click restart into the new version
- Pages can only open windows or your browser after a click, and only for the account you are looking at
