## Download and install

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

### Updating

An Atrium in the Applications folder updates itself: Settings > About shows new versions and
**Restart and update** installs them.

### Linux and Windows

| System | File |
|---|---|
| Debian, Ubuntu | `atrium-linux-amd64.deb` (`sudo apt install ./atrium-linux-amd64.deb`) |
| Other Linux | `atrium-linux-amd64.AppImage` (make it executable and run it) |
| Windows 10/11 | `atrium-windows-x64-setup.exe` (unsigned: SmartScreen > **More info** > **Run anyway**) |

These versions do not update themselves; Settings > About shows when a new one is out.
