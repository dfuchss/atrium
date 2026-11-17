# Building from source

Every platform needs [Rust](https://rustup.rs/) (stable) with the `wasm32-unknown-unknown` target, [Trunk](https://trunkrs.dev/) and the [Tauri CLI](https://v2.tauri.app/reference/cli/):

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk tauri-cli --locked   # or on macOS: brew install trunk

git clone https://github.com/<<REPO>>.git
cd atrium/app/src-tauri
```

Then follow the section of your platform. `cargo tauri dev` runs Atrium with a live-reloading UI everywhere. The checks every commit must pass are in [Checks](Checks).

## macOS

Release builds need macOS 14 or later: per-account data stores need WebKit's `WKWebsiteDataStore(forIdentifier:)`.

```sh
cargo tauri build                        # → target/release/bundle/macos/Atrium.app
cargo tauri build --debug                # → target/debug/bundle/macos/Atrium.app
```

Copy `Atrium.app` to `/Applications`. The app is not signed; the first time, open it as described in [macOS](macOS#install).

## Linux

Install the development packages first:

```sh
# Debian/Ubuntu
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev
# Fedora
sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel
```

Then build or run:

```sh
cargo tauri build                        # → target/release/bundle/appimage/atrium_<version>_<arch>.AppImage
cargo tauri build --debug --no-bundle    # only the binary: target/debug/atrium
```

To run the AppImage, see [Linux](Linux#install).

### The AppImage

The AppImage is configured by `app/src-tauri/tauri.linux.conf.json` (Linux only; the macOS bundle is unaffected) and contains a desktop entry and icon. It bundles the WebKitGTK and GTK libraries of the build machine, so build on the oldest distribution you want to support. It also bundles the build machine's GStreamer plugins, which WebKitGTK needs for audio and video (the libav decoders of `gstreamer1.0-libav` play H.264 video and AAC audio).

Building it needs these plugins, the tools that copy them and the tools `scripts/linux-check.sh --appimage` checks it with:

```sh
# Debian/Ubuntu
sudo apt-get install -y gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav patchelf file xxd jq
# Fedora (gstreamer1-libav from RPM Fusion free)
sudo dnf install gstreamer1-plugins-base gstreamer1-plugins-good gstreamer1-libav patchelf file xxd jq
```

On Fedora, remove `gstreamer1-plugins-bad-free`, a weak dependency of `webkit2gtk4.1`, or the plugin check of `scripts/linux-check.sh` rejects the AppImage.

### Containers and the Linux checks

On immutable distributions, build inside a distrobox or toolbox container. `scripts/linux-check.sh` runs the Linux checks (formatting, clippy, tests) on a Linux host or in such a container, e.g. `distrobox enter atrium-dev -- scripts/linux-check.sh`. With `--appimage` it also builds the AppImage, checks its desktop entry, icon and binary, and starts it under `xvfb-run` for a few seconds if that is installed. Set `CARGO_TARGET_DIR` to keep the Linux build apart from a macOS `target/` folder on a shared checkout.

## Windows

Atrium builds on Windows 10 and 11 (x86_64). Besides Rust, Trunk and the Tauri CLI, it needs:

- the [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (part of Windows 11 and of current Windows 10),
- the [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with *Desktop development with C++*, and Rust with the MSVC toolchain,
- [Git for Windows](https://gitforwindows.org/): the scripts run in its Git Bash.

Then build or run, in Git Bash:

```sh
cargo tauri build --debug --no-bundle    # only the binary: target/debug/atrium.exe
cargo tauri build --no-bundle            # the release executable: target/release/atrium.exe
```

The release is that `target\release\atrium.exe`, without an installer, renamed to `atrium-windows-x86_64.exe`. It updates itself once it runs outside Cargo's `target\` folder (which holds Cargo's `CACHEDIR.TAG`).

`scripts/check.sh` runs the checks in Git Bash as in CI. `.gitattributes` keeps every file at LF line ends in a Windows checkout too: the scripts and the tests need them.

A development instance next to an installed Atrium needs its own identifier on Windows; see [Debug options](Debug-Options#a-second-instance-on-windows).
