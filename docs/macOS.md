# Atrium on macOS

Atrium runs on macOS 14 or later, on Apple silicon and Intel Macs. It uses the system's WebKit; each account has a WebKit data store of its own.

## Install

Download the file for your Mac from the [latest release](https://github.com/<<REPO>>/releases/latest):

| Mac | File |
|---|---|
| Apple silicon | `atrium-macos-arm64.zip` |
| Intel | `atrium-macos-x86_64.zip` |

`SHA256SUMS` holds the checksums (`shasum -a 256 -c SHA256SUMS`).

The builds are not signed with an Apple code-signing certificate, so macOS blocks the first start of Atrium. You only need to allow it once:

1. Unzip the file and move **Atrium.app** into your **Applications** folder.
2. Start Atrium. macOS blocks it, saying that Apple cannot check it for malicious software. Click **Done**.
3. Open **System Settings** > **Privacy & Security** and scroll down to the **Security** section.
4. Click **Open Anyway** next to the note about Atrium, then confirm with your login password.

The **Open Anyway** button only shows for about an hour after the failed attempt. If it is gone, start Atrium again.

If macOS reports the app as "damaged and can't be opened", the app is not corrupt: macOS marks downloaded apps it cannot verify with a quarantine flag. Clear it once:

```sh
xattr -cr /Applications/Atrium.app
```

To build Atrium yourself, see [Building](Building#macos).

## Updates

An Atrium directly in `/Applications` or `~/Applications` updates itself (see [Updates](Usage#updates)) when you may replace `Atrium.app` there: Settings → About shows new versions, and *Restart and update* quits Atrium, swaps in the new version and starts it again. Elsewhere, Atrium just points to the release page; this includes an `Atrium.app` that macOS runs from a temporary location because it was started without moving it first (move it to Applications).

## Desktop integration

The keyboard shortcuts are in the app menu (see [Usage](Usage#keyboard-shortcuts)). The Dock badge and the menu bar icon show the total unread count. *After waking from sleep* (Settings → General, also per account) checks or reloads the services when you wake the Mac, not during Power Nap background wakes.

## Data locations

- **Config:** `~/Library/Application Support/org.fuchss.atrium/` (`accounts.json`, `services/`)
- **Log:** `~/Library/Logs/org.fuchss.atrium/atrium.log` (see [Privacy and security](Privacy-and-Security) for what it contains)
- **Web data (per account):** `~/Library/WebKit/org.fuchss.atrium/`

## Known limitations

- **Passkeys/WebAuthn don't work** inside embedded WebKit views for third-party sites. Apple restricts this to apps with a special browser entitlement. Use password + TOTP or SSO instead.

See also the [known limitations](Known-Limitations) of every platform.
