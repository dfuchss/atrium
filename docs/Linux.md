# Atrium on Linux

Atrium runs on Linux with WebKitGTK 4.1. Each account keeps its web data in a folder of its own. WebKit's web process sandbox is not available on Linux builds (Settings → About says so).

## Install

Atrium comes in two forms: the AppImage, which brings its own libraries (about 145 MB), and a plain binary, a fraction of that size, which uses the libraries of your system. Download one for your machine from the [latest release](https://github.com/<<REPO>>/releases/latest):

| Machine | AppImage | Binary |
|---|---|---|
| 64-bit Intel/AMD | `atrium-linux-x86_64.AppImage` | `atrium-linux-x86_64` |
| 64-bit ARM | `atrium-linux-aarch64.AppImage` | `atrium-linux-aarch64` |

`SHA256SUMS` holds the checksums (`sha256sum -c SHA256SUMS --ignore-missing`).

### AppImage

The AppImage brings its own WebKitGTK and needs glibc 2.39 or newer (e.g. Ubuntu 24.04, Debian 13, Fedora 40); it plays H.264 and AAC videos. Make it executable and start it:

```sh
chmod +x atrium-linux-x86_64.AppImage
./atrium-linux-x86_64.AppImage
```

AppImages need FUSE 2 (`libfuse2` on Debian/Ubuntu, `fuse` on Fedora); without it, start Atrium with `--appimage-extract-and-run`.

### Binary

The binary needs WebKitGTK 4.1, GTK 3, the Ayatana AppIndicator library and GStreamer on your system. It is built on Ubuntu 24.04, so it needs glibc 2.39 or newer and a distribution with WebKitGTK 4.1 (e.g. Ubuntu 24.04, Debian 13, Fedora 40). Install the packages, then make the file executable and put it in a folder of your own, for example `~/.local/bin`:

| Distribution | Packages |
|---|---|
| Debian, Ubuntu | `libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 gstreamer1.0-plugins-good gstreamer1.0-libav` |
| Fedora | `webkit2gtk4.1 gtk3 libappindicator-gtk3 gstreamer1-plugins-good gstreamer1-libav` (`gstreamer1-libav` is in RPM Fusion) |
| Arch | `webkit2gtk-4.1 gtk3 libayatana-appindicator gst-plugins-good gst-libav` |

```sh
install -Dm755 atrium-linux-x86_64 ~/.local/bin/atrium
atrium
```

`gstreamer1.0-libav`, `gstreamer1-libav` and `gst-libav` provide the H.264 and AAC decoders; without them such videos do not play. The AppImage trades size for independence: it runs where these libraries are missing or older, the binary is small and uses the security updates of your distribution's WebKitGTK.

To build Atrium yourself, see [Building](Building#linux).

## Updates

An Atrium started as an AppImage or as the release binary updates itself (see [Updates](Usage#updates)) when the file lies in a folder you can write to (e.g. `~/Applications` for the AppImage, `~/.local/bin` for the binary): Settings → About shows new versions, and *Quit and update* only quits Atrium and swaps in the new file under the same file name; open Atrium again afterwards. A binary from a `cargo` build folder (`target/`) and builds from source just point to the release page.

## Desktop integration

The window icon and launcher entry come from a desktop entry that matches the window's app id `org.fuchss.atrium`: by its name, `org.fuchss.atrium.desktop`, or by `StartupWMClass=org.fuchss.atrium`. The AppImage contains one (`atrium.desktop`, matched by its `StartupWMClass`), which an AppImage manager such as Gear Lever installs.

Without one, and for the binary, open Settings → General → Desktop integration in the running AppImage or binary and press **Install**: it writes `~/.local/share/applications/org.fuchss.atrium.desktop` and the icons `org.fuchss.atrium.png` below `~/.local/share/icons/hicolor/`. **Update** points the entry at the AppImage or binary again after you moved it, and **Uninstall** removes the entry and these icons.

Starting Atrium again brings the running Atrium to the front instead of opening a second one. The keyboard shortcuts work with <kbd>Ctrl</kbd> (see [Usage](Usage#keyboard-shortcuts)). The settings say tray icon, launcher badge and desktop light or dark setting where macOS has menu bar, Dock badge and macOS. *After waking from sleep* checks or reloads the services once the system resumes from suspend.

## Data locations

- **Config:** `~/.config/org.fuchss.atrium/` (`accounts.json`, `services/`)
- **Web data (per account):** `~/.local/share/org.fuchss.atrium/webdata/<account id>/`. WebKitGTK keeps the cookies there as plain text, and its content processes run without WebKit's bubblewrap sandbox, so a page that breaks out of one could read the web data of every account. The WebKit bundled in the AppImage looks for `bwrap` inside the AppImage and stops Atrium if the sandbox is turned on.
- **Log:** the app log folder, `~/.local/share/org.fuchss.atrium/logs/atrium.log`

`~/.config` and `~/.local/share` stand for `$XDG_CONFIG_HOME` and `$XDG_DATA_HOME` where these are set; the launcher entry and icons of *Install* go below `$XDG_DATA_HOME` too.

## Known limitations

- JavaScript dialogs name the host of the page, not of an embedded frame that asks (WebKitGTK does not tell which frame asked).
- There is no app menu; the keyboard shortcuts work with <kbd>Ctrl</kbd> instead of <kbd>⌘</kbd>. They take precedence over a page's own shortcuts with the same keys (for example <kbd>Ctrl</kbd><kbd>R</kbd> or <kbd>Ctrl</kbd><kbd>1</kbd> in a web app).
- The unread count appears as the tray label only where the panel supports it. The launcher badge works only where the desktop supports the Unity launcher API.
- Settings → Links → Browser lists the installed browsers; a command name saved by an earlier build is not recognised and links open in the system default browser.
- *Auto* appearance starts light and follows the desktop's light or dark colour scheme only once it changes while Atrium runs ([tao#1378](https://github.com/tauri-apps/tao/issues/1378)); such a change also overrides Light or Dark ([tao#1382](https://github.com/tauri-apps/tao/issues/1382)). A GTK theme named `*-dark` also keeps pages dark when Light is selected.
- On Wayland, starting Atrium again may only highlight the running Atrium in the task bar instead of bringing its window to the front.
- Session cookies are stored unencrypted in the per-account web data folder (mode `0700`): exclude `webdata/` from backups and sync.

See also the [known limitations](Known-Limitations) of every platform.
