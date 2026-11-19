# Atrium on Windows

Atrium runs on 64-bit Windows 10 and 11 (Intel/AMD) with WebView2, Microsoft's Chromium-based web view. Windows support is still in progress.

## Install

Download `atrium-windows-x86_64.exe` from the [latest release](https://github.com/<<REPO>>/releases/latest). It needs the [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/), which Windows 11 and current Windows 10 include. `SHA256SUMS` holds the checksums (`sha256sum -c SHA256SUMS --ignore-missing` in Git Bash, or compare with `Get-FileHash atrium-windows-x86_64.exe` in PowerShell).

Atrium is a single program file without an installer: move it to a folder of your own, for example `%LOCALAPPDATA%\Programs\Atrium`, and start it from there.

The build is not signed, so Windows SmartScreen warns on the first start of a downloaded copy ("Windows protected your PC"): click **More info**, then **Run anyway**. You only need to do this once. The browser marks the download as coming from the internet, which is what SmartScreen checks; the updates Atrium downloads itself are not marked and start without the warning.

To build Atrium yourself, see [Building](Building#windows).

## Updates

A release build updates itself (see [Updates](Usage#updates)) when the executable lies in a folder you can write to (e.g. `%LOCALAPPDATA%\Programs\Atrium`): Settings → About shows new versions, and *Quit and update* quits Atrium and swaps in the verified new executable under the same file name (the old one is removed when Atrium starts again); open Atrium again afterwards. Development builds and builds that run from Cargo's `target` folder just point to the release page.

## Desktop integration

Atrium adds nothing to the Start menu by itself. To get a Start menu entry, open Settings → General → Desktop integration and press **Install**: it writes the shortcut `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Atrium.lnk` to the running program, with Atrium's identifier `org.fuchss.atrium` as app id, so Windows shows Atrium's name on its notifications. **Update** points the shortcut at the program again after you moved it, and **Uninstall** removes the shortcut (and nothing else).

Atrium's windows carry the same app id, so the taskbar groups them under that shortcut, with its name and icon. The keyboard shortcuts work with <kbd>Ctrl</kbd> (see [Usage](Usage#keyboard-shortcuts)); Settings and Quit are also in the tray menu. *Auto* appearance follows the light or dark app mode of Windows, also when it changes while Atrium runs. *After waking from sleep* checks or reloads the services once the system resumes from sleep or hibernation.

New accounts present Chrome's user agent instead of Safari's, as WebView2 is Chromium; it can be changed per account.

## Data locations

- **Config:** `%APPDATA%\org.fuchss.atrium\` (`accounts.json`, `services\`)
- **Log:** `%LOCALAPPDATA%\org.fuchss.atrium\logs\atrium.log`
- **Web data:** `%LOCALAPPDATA%\org.fuchss.atrium\`: the sidebar's in `EBWebView\`, each account's in `webdata\<account id>\EBWebView\`. Every account folder runs a WebView2 browser process of its own, which costs memory per account.

Atrium finds these folders through Windows (`SHGetKnownFolderPath`), so changing `%APPDATA%` or `%LOCALAPPDATA%` does not move them.

## Known limitations

- A JavaScript dialog opens in a small window above the window that asks; it is not modal, the rest of Atrium stays usable while the page waits for the answer.
- There is no app menu; Settings and Quit are in the tray menu. The keyboard shortcuts work while a webview of the main window has the focus, not in popups or dialog windows, and count keys by their place on a US keyboard (<kbd>Ctrl</kbd><kbd>[</kbd> and <kbd>Ctrl</kbd><kbd>]</kbd> are the two keys right of P).
- The taskbar button shows a red dot for unread messages, not their count; the tray icon shows no count.
- Pages may open windows without a click (the window quota still applies); WebView2 has no popup blocker.
- A service whose page stopped responding is not reloaded, only logged; a service whose WebView2 browser process ended (all processes of its account) stays blank until Atrium starts again. A crashed or ended page process is reloaded, at most three times a minute.
- Hidden services keep running, but Chromium throttles their timers like those of a background tab (Tauri's background throttling setting does not apply to WebView2).
- Notifications of a build in `target\debug` or `target\release` are shown under the name of PowerShell. Elsewhere, Windows shows Atrium's name once the Start menu shortcut is installed (see [Desktop integration](#desktop-integration)), which carries the identifier as app id (a development identifier for a development instance).
- A click on a notification shows its account only while the notification is on screen, not once it moved to the notification centre.
- *Do not disturb* also mutes a call held in an in-app window: WebView2 does not tell when a page uses the microphone or camera, and only the service itself reports it. A call in the service keeps its sound.

See also the [known limitations](Known-Limitations) of every platform.
