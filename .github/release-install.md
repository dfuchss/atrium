## Download and install

### macOS

Unsigned macOS builds of Atrium (macOS 14 or later). Pick the file for your Mac:

| Mac | File |
|---|---|
| Apple silicon | `atrium-macos-arm64.zip` |
| Intel | `atrium-macos-x86_64.zip` |

`SHA256SUMS` holds the checksums (`shasum -a 256 -c SHA256SUMS`).

> **These builds are not signed** with an Apple code-signing certificate, so macOS blocks the first start of
> Atrium. The steps below tell macOS to go ahead. You only need to do this once.

1. Unzip the file and move **Atrium.app** into your **Applications** folder.
2. Start Atrium. macOS blocks it, saying that Apple cannot check it for malicious software. Click **Done**.
3. Open **System Settings** > **Privacy & Security** and scroll down to the **Security** section.
4. Click **Open Anyway** next to the note about Atrium, then confirm with your login password.

The **Open Anyway** button only shows for about an hour after the failed attempt. If it is gone, start Atrium again.

<details>
<summary>macOS says Atrium is "damaged and can't be opened"</summary>

The app is not corrupt. macOS marks downloaded apps it cannot verify with a quarantine flag. Clear it once:

```bash
xattr -cr /Applications/Atrium.app
```

</details>

### Linux

`atrium-linux-x86_64.AppImage` runs on 64-bit Intel/AMD Linux and `atrium-linux-aarch64.AppImage` on 64-bit ARM Linux; both bring their own WebKitGTK and need glibc 2.39 or newer (e.g. Ubuntu 24.04, Debian 13, Fedora 40). Make it executable and start it:

```bash
chmod +x atrium-linux-x86_64.AppImage
./atrium-linux-x86_64.AppImage
```

AppImages need FUSE 2 (`libfuse2` on Debian/Ubuntu, `fuse` on Fedora); without it, start Atrium with `--appimage-extract-and-run`. To get a launcher entry and the window icon, press **Install** under Settings > General > Desktop integration (**Uninstall** removes it again), or integrate the AppImage with an AppImage manager such as Gear Lever. `SHA256SUMS` holds the checksums (`sha256sum -c SHA256SUMS --ignore-missing`).

### Updating

On macOS, an Atrium in the Applications folder updates itself: Settings > About shows new versions and
**Restart and update** installs them.

On Linux, the AppImage updates itself when it lies in a folder you can write to (e.g. `~/Applications`):
Settings > About shows new versions and **Quit and update** swaps in the new AppImage; open Atrium again
afterwards.
