# Changelog

All notable changes to Atrium are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow
[Semantic Versioning](https://semver.org/) (`X.Y.Z`); a release is tagged `vX.Y.Z`. Each version starts
with a few lines of highlights and, where needed, upgrade notes; the sections below are grouped by
platform (see [Releasing](https://github.com/dfuchss/atrium/wiki/Releasing#changelog)).

## [Unreleased]

### Added

#### Linux

- A plain release binary, `atrium-linux-x86_64` and `atrium-linux-aarch64`, next to the AppImages: it is much smaller and uses the system's WebKitGTK 4.1, GTK 3, libayatana-appindicator and GStreamer. It updates itself in a folder you can write to and offers *Desktop integration* in the settings like the AppImage; see the [Linux](https://github.com/dfuchss/atrium/wiki/Linux) page for the packages it needs

### Changed

#### All platforms

- Release builds are optimised for size (link-time optimisation, stripped symbols), so the downloads are smaller

## 0.6.2

On Windows, a click on a notification opens its account also after the notification moved to the notification centre.

### Fixed

#### Windows

- A click on a notification in the notification centre shows its account, not only while the notification is on screen

## 0.6.1

*Do not disturb* now also mutes the services' own sounds, so their notification sounds stay quiet too. A service in a call keeps its sound.

### Fixed

#### All platforms

- *Do not disturb* also mutes the sounds of the services and their in-app windows, except for a service in a call

#### Windows

- A call in an in-app window is muted by *Do not disturb* as well; a call in the service itself keeps its sound

## 0.6.0

Settings can be exported and imported: everything into a new installation, or only the settings, custom services and service definition files into an existing one, keeping its services and logins. Each service can override the sidebar badge style and the dock badge. Slack shows its unread count and dot again.

> **Upgrade notes:** the per-service *Count in dock badge* switch is gone. A service that had it off now uses the global dock badge setting; set its *Dock badge* to *Off* to exclude it again.

### Added

#### All platforms

- Each service can override the sidebar badge style (*Count*, *Dot*, *Urgent only*) in its settings; *Global* keeps the default
- Each service can override the *Dock badge* setting (*Unread count and dot*, *Unread count only*, *Off*) in its settings; *Global* keeps the default
- The *Export and import* tab saves everything (accounts, settings, custom services and service definition files) or the settings only to a file you choose; the file is readable by you only and holds no logins
- *Import everything* takes a whole export into an Atrium without accounts and restarts it right after saving; *Import settings* takes the settings and custom services of an export and adds its service definition files, keeping your accounts and existing files

### Changed

#### All platforms

- *Settings…* in a service's menu now reliably opens the settings dialog on that service, also when the dialog is closed, and scrolls to it
- The badge settings of a service use the same segmented buttons as the global settings
- The *Sidebar badges* setting moved from *General* to *Notifications*
- A custom service whose id equals a built-in service or a service definition file is logged when it replaces that service

### Fixed

#### All platforms

- Slack shows its unread count and dot again: the count comes from Slack's current title ("2 new items"), and an unread channel that is not muted shows the dot

## 0.5.0

Sidebar badges tell mentions and direct messages (a red count) from other unread messages (a white dot), with a setting for each style and for muted accounts. Right-clicking a service opens a menu to put it to sleep, open its settings, turn its notifications on or off, or reload it. External links from the Linux AppImage now also open on Fedora-like hosts.

### Added

#### All platforms

- Right-clicking a service in the sidebar opens its menu: *Sleep* or *Wake Up*, *Settings…* (opens the service's settings), *Enable Notifications* or *Disable Notifications*, and *Reload*; the shown service cannot be put to sleep, and waking one up loads it in the background
- Settings > Notifications > *Badge when muted* decides if an account with notifications off keeps its unread badge on the sidebar icon (on by default)
- Sidebar badges show mentions and direct messages in red and other unread messages as a white dot; choose a count or a dot in Settings > General
- *Urgent only* in Settings > General > Sidebar badges shows only the red count of mentions and direct messages and hides the white dot (Element counts all its notified messages as urgent)

### Fixed

#### All platforms

- An account with notifications off no longer notifies you of finished downloads
- Opening a service definition file from the service list for editing now shows the error when the file cannot be read, instead of doing nothing

#### Linux

- External links open from the AppImage on hosts that are not Ubuntu-like, such as Fedora, instead of failing: Atrium now uses the host's `gio-launch-desktop`

## 0.4.0

Atrium comes to Windows: an unsigned, self-updating executable for 64-bit Windows 10 and 11, with Atrium's own JavaScript dialogs, the keyboard shortcuts, wake and crash recovery, a browser choice and a Start menu entry. Element's unread count now includes the spaces when Home shows only some rooms.

### Added

#### Windows

- Releases include an unsigned x86_64 executable, `atrium-windows-x86_64.exe`: a single file without an installer
- Pushes and pull requests get a Windows debug executable among the run's artifacts
- A release build updates itself: *Quit and update* swaps in the verified new executable under the same file name after Atrium has quit; open Atrium again afterwards. It must lie in a folder you can write to; builds that run from Cargo's target folder just point to the release page
- Settings > General > Desktop integration installs or removes a Start menu shortcut to the running Atrium (*Install*, *Update*, *Uninstall*); it carries Atrium's identifier as app id, so Windows shows Atrium's name on its notifications
- Atrium's windows carry its app id (`org.fuchss.atrium`), so the taskbar groups them under a Start menu shortcut with that id, with its name and icon
- `alert`, `confirm` and `prompt` of service pages and their sign-in popups show Atrium's own dialogs, in a small window in Atrium's style: named after the account and site, with suppression and a rate limit; before-unload prompts no longer ask
- The keyboard shortcuts of the macOS menu work with Ctrl, as on Linux: settings, reload, back and forward, zoom (also Ctrl++ and Ctrl with the numeric keypad's +, - and 0), open in browser, close, quit and switching accounts with Ctrl+1 to Ctrl+9; keys count by their place on a US keyboard, as in Chrome and Edge
- *Force reload* reloads the service without its cache, as on macOS and Linux
- The *Auto* appearance follows the light or dark app mode of Windows, also when it changes while Atrium runs
- *After waking from sleep* works too: services are checked or reloaded once the system resumes from sleep or hibernation
- A service whose page process crashed or was ended is reloaded instead of showing WebView2's error page, at most three times a minute as on macOS and Linux
- Settings > Links > Browser lists the installed browsers to open links in; a browser that cannot be started falls back to the system default, and mail links open in the default mail app
- New accounts present Chrome's user agent instead of Safari's, as WebView2 is Chromium; it can still be changed per account

### Changed

#### All platforms

- Element's unread count adds the badges of the top-level spaces when Home shows only some rooms (*Show all rooms in Home* off), and is at least the number of unread rooms in the title

### Fixed

#### All platforms

- The *Add* button in Settings > Accounts no longer moves when the *Added* note appears: the note shows in the empty label column beside the button, and the add form's rows span its full width like those of the other settings

## 0.3.0

Atrium runs on Linux: AppImages for x86_64 and aarch64 that update themselves, with Atrium's own JavaScript dialogs, the keyboard shortcuts with Ctrl, desktop integration, wake and crash recovery and a browser choice. Atrium also has a new app icon.

> **Upgrade notes:** the `safari` user agent preset is renamed `webkit`: services and accounts that set `user_agent = "safari"` must use `"webkit"`.

### Added

#### All platforms

- A page that keeps crashing is reloaded at most three times a minute and then left until you reload it

#### Linux

- Atrium runs on Linux (WebKitGTK 4.1) when built from source; each account keeps its web data in a folder of its own; WebKit's web process sandbox is not available on Linux builds (Settings > About says so)
- Releases include AppImages for x86_64 and aarch64 (`atrium-linux-x86_64.AppImage`, `atrium-linux-aarch64.AppImage`); they play H.264 and AAC videos
- `cargo tauri build` produces an AppImage with a desktop entry and icon
- The AppImage updates itself: *Quit and update* swaps in the verified new version after Atrium has quit; open Atrium again afterwards
- Settings > General > Desktop integration installs or removes the launcher entry and icons of the running AppImage (*Install*, *Update*, *Uninstall*)
- Starting Atrium again brings the running Atrium to the front instead of opening a second one
- `alert`, `confirm` and `prompt` of service pages show Atrium's own dialogs, as on macOS: named after the account and site, with suppression and a rate limit; before-unload prompts no longer ask
- The keyboard shortcuts of the macOS menu work with Ctrl: settings, reload, back and forward, zoom (also Ctrl++ and Ctrl with the numeric keypad's +, - and 0), open in browser, close, quit and switching accounts with Ctrl+1 to Ctrl+9
- The *Auto* appearance follows changes of the desktop's light or dark colour scheme while Atrium runs
- Settings > Links > Browser lists the installed browsers to open links in; a browser that is no longer installed falls back to the system default, and mail links open in the default mail app
- *After waking from sleep* works too: services are checked or reloaded once the system resumes from suspend
- A service whose web process crashed or ran out of memory is reloaded instead of staying blank
- The settings say tray icon, launcher badge and desktop light or dark setting instead of menu bar, dock badge and macOS

### Changed

#### All platforms

- New app icon: an atrium courtyard with arched walls around a lit floor

#### Linux

- The WebKit and Chrome presets report a Linux platform

### Fixed

#### All platforms

- The *Added* note after adding an account appears beside the *Add* button instead of below it, so the settings no longer jump

#### macOS

- A woken account no longer keeps a small yellow remnant of its sleeping moon on the sidebar icon

#### Linux

- Drop-down lists draw their own arrow, so their text is no longer cut off

## 0.2.0

Services can start asleep and be force-reloaded without the cache. After a wake, Atrium reloads only the services that stopped responding, and downloads never replace an existing file.

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

First release, for macOS: all your messengers in one window, each account in its own isolated web view, with notifications, unread badges, sleep, custom services and automatic updates.

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
