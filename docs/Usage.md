# Usage

To install Atrium, see [macOS](macOS#install), [Linux](Linux#install) or [Windows](Windows#install).

## Accounts

**Add accounts** with the **+** button at the bottom of the sidebar: choose the service, a label and, for services such as Element or Mastodon, the instance (the server host). The settings dialog stays open, so you can add several in a row. Drag icons in the sidebar to reorder them, or use *Move up* / *Move down* in the account list.

**Right-click** a service icon in the sidebar for its menu: *Sleep* or *Wake Up*, *Settings…*, *Enable Notifications* or *Disable Notifications*, and *Reload*. The service on screen cannot be put to sleep.

Each account in Settings → Accounts has these actions: *Edit*, *Reload*, *Remove* (asks to confirm and deletes the account's web data), *Open in browser*, *Back*, *Forward*, *Sleep now* (unloads the view to save memory) and *Clear website data…* (logs you out of the service).

## Settings

Open the settings with the gear button or <kbd>⌘</kbd><kbd>,</kbd> (<kbd>Ctrl</kbd><kbd>,</kbd> on Linux and Windows).

### Account settings

*Edit* on an account in the Accounts tab:

| Setting | What it does |
|---|---|
| Label | The account's name in the sidebar and in notifications. |
| Instance | Only for services that ask for one (its label comes from the service, e.g. *Element web host*). Changing it reloads the account. |
| Enabled | Disabled accounts keep their login but are not loaded. |
| Notifications | Off mutes the account: no notifications, and not counted in the badge. |
| Unread badge | The unread badge on the sidebar icon. |
| Count in dock badge | Count the account in the Dock badge (*launcher badge* on Linux, *taskbar badge* on Windows). |
| Open links in | Where links that open a new window go: *Use global setting*, *System browser* or *App window*. |
| Icon | *Use site icon* or *Refresh site icon*; a [Simple Icons](https://simpleicons.org/) slug (a brand glyph on the service colour); or an image URL or file. Precedence: custom image, Simple Icon, site icon, initials. |
| Camera & microphone | *Service default*, *Allow* or *Block*; also covers screen capture, and only on the service's own hosts (see [Service definitions](Service-Definitions#hosts)). |
| Zoom | 50 % to 200 %. |
| Sleep in background | Put the account to sleep after 5, 15, 30, 60 or 120 minutes in the background, *Never*, or the global setting. |
| Start asleep | Don't load the account when Atrium starts; it loads when you select it. Takes effect at the next start. |
| After waking from sleep | *Check, reload if stuck*, *Reload* or *Do nothing*, or the global setting (see General). |
| Force reload | Reloads the page without the cache, for a service that stays broken after a normal reload. |
| User agent | *Service default*, *WebKit browser*, *Chrome* or a custom user agent. Changing it reloads the account. |
| Always open in the app | Hosts whose new windows open inside the app with the account's login, e.g. a single-sign-on provider such as `idp.example.org` or `*.microsoftonline.com`. |
| Custom CSS | Injected into every page of the account; changing it reloads the account. |

### Custom services

Custom services have a name, a URL (with `{instance}` for the part that differs per account), a colour, an optional instance field label, an optional unread title pattern (capture group 1 is the count; without one, a title that starts with `(n)` counts) and a user agent (*WebKit browser*, the default, *Chrome* or custom). *Start from* fills the fields from a built-in service.

Below them, *Service definition files* lists the TOML files of the services folder: *Open folder*, *Reload*, *New definition*, and per file *Edit*, *Disable*/*Enable* and *Delete*. See [Service definitions](Service-Definitions).

### General

| Setting | What it does |
|---|---|
| Sidebar icon size | Small, medium or large icons; the sidebar width follows. |
| Appearance | *Auto*, *Light* or *Dark*. Auto follows the system; service pages follow if they support it. |
| Launch at login | Start Atrium when you log in. |
| Start hidden | Start without showing the window. |
| Keep running in background | Closing the window keeps Atrium running; otherwise it quits. |
| Menu bar icon | The menu bar icon on macOS with the unread count and the account menu (*Tray icon* on Linux and Windows). |
| Put inactive services to sleep after | 5, 15, 30, 60 or 120 minutes, or *Never*. Sleeping services free memory but show no notifications or unread counts until you open them again. |
| After waking from sleep | *Check, reload if stuck* reloads only the services that stop responding after a wake, *Reload* reloads all, *Do nothing*. When this applies differs per platform: [macOS](macOS#desktop-integration), [Linux](Linux#desktop-integration), [Windows](Windows#desktop-integration). |
| Desktop integration | Linux and Windows only: install, update or remove the launcher entry ([Linux](Linux#desktop-integration)) or Start menu shortcut ([Windows](Windows#desktop-integration)). |

### Notifications

*Do not disturb* suppresses all notifications. *Message preview* shows the message text (otherwise only "New message"), *Play sound* plays the notification sound. *Badge when muted* keeps or hides the sidebar badge of accounts with notifications off.

### Dock badge

Called *Launcher badge* on Linux and *Taskbar badge* on Windows. *Show*: *Unread count and dot* (a dot when only indirect unread exists), *Unread count only* or *Off*. Accounts can be left out in their settings.

### Links

*New-window links* go to the *System browser* or an *App window* that shares the account's login; accounts can override it. Sign-in popups always stay in the app, so OAuth and SSO keep working. *Browser* picks the browser: on macOS the name of an installed application (e.g. Firefox), on Linux and Windows one of the installed browsers that Atrium lists. Without a choice, links open in the system default browser.

### About

The version, the project page, the config, log and services folders, and the updates: status, *Check now*, *Release notes*, *Check automatically* and the button that installs an update (see [Updates](#updates)). On Linux it also says that the web process sandbox is not available.

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| <kbd>⌘</kbd><kbd>,</kbd> | Open the settings |
| <kbd>⌘</kbd><kbd>1</kbd> … <kbd>⌘</kbd><kbd>9</kbd> | Switch to account 1–9 |
| <kbd>⌘</kbd><kbd>R</kbd> | Reload the current account |
| <kbd>⌘</kbd><kbd>[</kbd> / <kbd>⌘</kbd><kbd>]</kbd> | Back / forward |
| <kbd>⌘</kbd><kbd>=</kbd> / <kbd>⌘</kbd><kbd>-</kbd> / <kbd>⌘</kbd><kbd>0</kbd> | Zoom in / out / reset (per account) |
| <kbd>⌘</kbd><kbd>⇧</kbd><kbd>O</kbd> | Open the current page in the browser |
| <kbd>⌘</kbd><kbd>W</kbd> | Close the window (Atrium keeps running in the background) |
| <kbd>⌘</kbd><kbd>Q</kbd> | Quit |

- **macOS:** the shortcuts are in the app menu, which also has the standard items: Hide and Hide Others, the Edit menu (undo, redo, cut, copy, paste, select all), full screen, Minimize and Close Window.
- **Linux and Windows:** there is no app menu. Use <kbd>Ctrl</kbd> where the table says <kbd>⌘</kbd> (and <kbd>Shift</kbd> for <kbd>⇧</kbd>); they work while a service has the focus. <kbd>Ctrl</kbd><kbd>+</kbd> and <kbd>Ctrl</kbd> with the numeric keypad's <kbd>+</kbd>, <kbd>-</kbd> and <kbd>0</kbd> zoom as well. The differences are under [Linux](Linux#known-limitations) and [Windows](Windows#known-limitations).

The tray (menu bar) menu has *Show Atrium*, the accounts with their unread counts, *Do Not Disturb*, *Settings…* and *Quit*.

## Updates

Atrium checks for a new release on startup and every six hours (Settings → About; *Check automatically* switches it off, *Check now* checks at once). An update is downloaded, verified against the release's `SHA256SUMS` and placed next to the app; the button in Settings → About installs it. A release without a file for your machine is only announced.

This works only for a release build in a location Atrium can update; development builds and other locations just point to the release page, and Settings → About says why. Where that is and what the button does differs per platform: [macOS](macOS#updates), [Linux](Linux#updates), [Windows](Windows#updates).
