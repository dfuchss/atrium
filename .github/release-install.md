## Download

| Machine | File |
|---|---|
| Mac with Apple silicon (macOS 14 or later) | [`atrium-macos-arm64.zip`](https://github.com/dfuchss/atrium/releases/download/{{tag}}/atrium-macos-arm64.zip) |
| Mac with Intel (macOS 14 or later) | [`atrium-macos-x86_64.zip`](https://github.com/dfuchss/atrium/releases/download/{{tag}}/atrium-macos-x86_64.zip) |
| Linux, 64-bit Intel/AMD (glibc 2.39 or later) | [`atrium-linux-x86_64.AppImage`](https://github.com/dfuchss/atrium/releases/download/{{tag}}/atrium-linux-x86_64.AppImage) |
| Linux, 64-bit ARM (glibc 2.39 or later) | [`atrium-linux-aarch64.AppImage`](https://github.com/dfuchss/atrium/releases/download/{{tag}}/atrium-linux-aarch64.AppImage) |
| Windows 10 and 11, 64-bit Intel/AMD | [`atrium-windows-x86_64.exe`](https://github.com/dfuchss/atrium/releases/download/{{tag}}/atrium-windows-x86_64.exe) |

[`SHA256SUMS`](https://github.com/dfuchss/atrium/releases/download/{{tag}}/SHA256SUMS) holds the checksums of all files; the commands to check them are in the steps below. An installed Atrium updates itself to this version from Settings > About. More in the [wiki](https://github.com/dfuchss/atrium/wiki).

## Install

### macOS

> **These builds are not signed** with an Apple code-signing certificate, so macOS blocks the first start of
> Atrium. The steps below tell macOS to go ahead. You only need to do this once.

1. Unzip the file and move **Atrium.app** into your **Applications** folder.
2. Start Atrium. macOS blocks it, saying that Apple cannot check it for malicious software. Click **Done**.
3. Open **System Settings** > **Privacy & Security** and scroll down to the **Security** section.
4. Click **Open Anyway** next to the note about Atrium, then confirm with your login password.

The **Open Anyway** button only shows for about an hour after the failed attempt. If it is gone, start Atrium again.
Checksums: `shasum -a 256 -c SHA256SUMS`.

<details>
<summary>macOS says Atrium is "damaged and can't be opened"</summary>

The app is not corrupt. macOS marks downloaded apps it cannot verify with a quarantine flag. Clear it once:

```bash
xattr -cr /Applications/Atrium.app
```

</details>

Atrium updates itself while it lies in the Applications folder: **Restart and update** in Settings > About.

### Linux

Both AppImages bring their own WebKitGTK; their glibc floor fits e.g. Ubuntu 24.04, Debian 13 and Fedora 40. Make the
AppImage executable and start it:

```bash
chmod +x atrium-linux-x86_64.AppImage
./atrium-linux-x86_64.AppImage
```

AppImages need FUSE 2 (`libfuse2` on Debian/Ubuntu, `fuse` on Fedora); without it, start Atrium with
`--appimage-extract-and-run`. To get a launcher entry and the window icon, press **Install** under Settings > General >
Desktop integration (**Uninstall** removes it again), or integrate the AppImage with an AppImage manager such as
Gear Lever. Checksums: `sha256sum -c SHA256SUMS --ignore-missing`.

The AppImage updates itself when it lies in a folder you can write to (e.g. `~/Applications`): **Quit and update**
swaps in the new AppImage; open Atrium again afterwards.

### Windows

Atrium needs the WebView2 Runtime, which Windows 11 and current Windows 10 include. It is a single program file
without an installer: move it to a folder of your own, for example `%LOCALAPPDATA%\Programs\Atrium`, and start it
from there.

> **This build is not signed**, so Windows SmartScreen warns on the first start of a downloaded copy
> ("Windows protected your PC"). Click **More info**, then **Run anyway**. You only need to do this once.

The browser marks the download as coming from the internet, which is what SmartScreen checks; the updates Atrium
downloads itself are not marked and start without the warning. To get a Start menu entry, press **Install** under
Settings > General > Desktop integration (**Uninstall** removes it again). Checksums:
`sha256sum -c SHA256SUMS --ignore-missing` in Git Bash, or compare with `Get-FileHash atrium-windows-x86_64.exe` in
PowerShell.

Atrium updates itself when it lies in a folder you can write to: **Quit and update** swaps in the new version under
the same file name; open Atrium again afterwards.
