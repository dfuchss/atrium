# Development

How to build, check and release Atrium. For using it, see the [README](README.md).

## Building from source

Requirements:
- macOS 14 or later (release builds). On Linux, see [Linux](#linux). Per-account data stores need WebKit's `WKWebsiteDataStore(forIdentifier:)`.
- [Rust](https://rustup.rs/) (stable) with the `wasm32-unknown-unknown` target.
- [Trunk](https://trunkrs.dev/) and the [Tauri CLI](https://v2.tauri.app/reference/cli/).

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk tauri-cli --locked   # or: brew install trunk

git clone https://github.com/dfuchss/atrium.git
cd atrium/app/src-tauri
cargo tauri build                        # → target/release/bundle/macos/Atrium.app
```

Copy `Atrium.app` to `/Applications`. The app is not signed; the first time, open it as described in the [README](README.md#installation).

### Linux

To build from source, install the development packages:

```sh
# Debian/Ubuntu
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev
# Fedora
sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel
```

Then install [Rust](https://rustup.rs/) with the `wasm32-unknown-unknown` target, [Trunk](https://trunkrs.dev/) and the [Tauri CLI](https://v2.tauri.app/reference/cli/) as above, and build or run:

```sh
cd atrium/app/src-tauri
cargo tauri build                        # → target/release/bundle/appimage/atrium_<version>_<arch>.AppImage
cargo tauri build --debug --no-bundle    # only the binary: target/debug/atrium
cargo tauri dev                          # run with live-reloading UI
```

The AppImage is configured by `app/src-tauri/tauri.linux.conf.json` (Linux only; the macOS bundle is unaffected). It bundles the WebKitGTK and GTK libraries of the build machine, so build on the oldest distribution you want to support. It also bundles the build machine's GStreamer plugins, which WebKitGTK needs for audio and video (the libav decoders of `gstreamer1.0-libav` play H.264 video and AAC audio); building it needs them, the tools that copy them and the tools `scripts/linux-check.sh --appimage` checks it with: `sudo apt-get install -y gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav patchelf file xxd jq` (Fedora: `gstreamer1-plugins-base gstreamer1-plugins-good gstreamer1-libav patchelf file xxd jq`, with `gstreamer1-libav` from RPM Fusion free; remove `gstreamer1-plugins-bad-free`, a weak dependency of `webkit2gtk4.1`, or the plugin check of `scripts/linux-check.sh` rejects the AppImage). To run it, see [Linux](README.md#linux) in the README.

On immutable distributions, build inside a distrobox or toolbox container. `scripts/linux-check.sh` runs the Linux checks (formatting, clippy, tests) on a Linux host or in such a container, e.g. `distrobox enter atrium-dev -- scripts/linux-check.sh`. With `--appimage` it also builds the AppImage, checks its desktop entry, icon and binary, and starts it under `xvfb-run` for a few seconds if that is installed. Set `CARGO_TARGET_DIR` to keep the Linux build apart from a macOS `target/` folder on a shared checkout.

## Architecture

Atrium follows a hexagonal design. A pure Rust core decides; adapters execute.

| Path | What |
|---|---|
| `crates/domain` | Pure core: service catalog, reducer `Core::handle(Input) -> Vec<Effect>`, port traits, effect runner. No Tauri, no IO. |
| `crates/protocol` | Types shared by the backend and the UI (`UiCommand`, `Snapshot`). Compiles to wasm. |
| `crates/host-tauri` | Adapter: implements the ports on Tauri multi-webview (webviews, notifications, dock badge, tray, menus, links); platform-specific code in `native/`. |
| `crates/store-fs` | Adapter: atomic JSON store (`accounts.json`). |
| `crates/fs-util` | File operations shared by the crates: atomic writes with a fixed mode. |
| `crates/updater` | Self-update from GitHub Releases: check, verify, stage next to the app, swap after quitting; the macOS app bundle or the Linux AppImage, platform-specific code in `native/`. |
| `crates/webview-dialogs` | Adapter: native `alert`/`confirm`/`prompt` panels for the service pages, with platform-neutral dialog types; on macOS added to the system webview's delegate at runtime, on Linux shown from WebKitGTK's `script-dialog` signal. The only crate built on `unsafe` code; elsewhere it is denied, except for one environment change at start-up on Linux in `host-tauri`. |
| `app/src-tauri` | Composition root: window, actor thread, Tauri commands. |
| `app/ui` | Sidebar and settings in [Leptos](https://leptos.dev/) (CSR, built with Trunk). |
| `services/*.toml` | Built-in service definitions. |

The web engine sits behind the `WebviewHost` port, so a Chromium (CEF) backend can be added later without touching the core.

## Checks

```sh
scripts/check.sh                               # everything CI checks: fmt, UI build, clippy, tests
cargo test --workspace --exclude atrium-ui     # core, adapters, store
cd app/src-tauri && cargo tauri dev            # run with live-reloading UI
cd app/src-tauri && cargo tauri build --debug  # bundle: target/debug/bundle/macos/Atrium.app
```

`scripts/check.sh` must pass before every commit; the GitHub workflow runs it too. The lint rules are in the `[workspace.lints]` section of `Cargo.toml` (clippy pedantic plus a few denials, each allowed exception with its reason), the thresholds for function length, complexity and arguments in `clippy.toml`.

## Releasing

Releases are built by `.github/workflows/release.yml` when a tag `v<version>` is pushed; the tag must match the version in the workspace `Cargo.toml`, the only place the version is set.

Notes for the user-visible changes go under `## [Unreleased]` in [`CHANGELOG.md`](CHANGELOG.md) as they happen. To release:
1. Rename the Unreleased content to a `## X.Y.Z` section and leave a fresh empty `## [Unreleased]` above it.
2. Set the version in the workspace `Cargo.toml`.
3. Commit and tag `vX.Y.Z`.

The release workflow publishes that section plus the install text from `.github/release-install.md` (built by `.github/scripts/release-notes.sh`) and fails if the section is missing or empty.

## Debug options

Debug builds (`cargo tauri dev`, `cargo tauri build --debug`) read these environment variables; release builds ignore them. They let a second, isolated instance run next to the regular one, e.g. for screenshots:

| Variable | Effect |
|---|---|
| `ATRIUM_CONFIG_DIR=<dir>` | Config folder instead of the app's: `accounts.json`, `services/` and `atrium.log`. Such an instance never changes the login item, and on Linux it starts even while Atrium runs. |
| `ATRIUM_ALWAYS_ON_TOP=1` | Keeps the main window on top. macOS does not paint occluded windows, so `screencapture -l` would capture a blank window. |
| `ATRIUM_WINDOW_POSITION=<x>,<y>` | Position of the main window in logical pixels (the main display starts at `0,0`), e.g. to open it on a Retina display. |
| `ATRIUM_DEV_INIT_SCRIPT=<file>` | JavaScript appended to the init script of every service view (the screenshot script seeds a demo Element session with it). |
| `ATRIUM_LOG_TITLES=1` | Logs page titles with each unread change, to check a service's `title_regex` against what it really shows. Titles name chats and senders, so this is off otherwise. |
| `ATRIUM_DEV_RELEASE_DIR=<dir>` | Folder that stands in for the latest GitHub release, to try an update by hand: `release.json` (the GitHub latest-release JSON; asset URLs must start with `https://github.com/dfuchss/atrium/releases/download/`, only their file name is used), `SHA256SUMS` and the asset. The debug build then counts as installed, so it updates itself when it runs from an installable location. |
| `ATRIUM_DEV_UI_QUERY=<query>` | Query string for the sidebar UI. `dialog=<tab>` opens the settings dialog on a tab (`accounts`, `general`, `notifications`, `dock`, `links`, `custom`, `about`), `edit=<index>` opens the edit form of an account, `addcustom=1` the custom service form, `editservice=<file>` the definition editor (demo files `wiki.toml`, `broken.toml`). |

## Screenshots

`scripts/screenshots.sh` takes the README screenshots in `docs/screenshots/` with demo data only. It runs a throwaway Matrix homeserver (tuwunel) and Element web in Docker, seeds demo users and chats, builds a debug bundle with its own bundle id `org.fuchss.atrium.demo` into `target/screenshots`, and starts it with the variables above and a temporary config folder. The own bundle id keeps the demo's WebKit data, logs, caches and WebCrypto Keychain item apart from a regular installation (sharing `org.fuchss.atrium` would make macOS ask for access to its Keychain item). At the end it removes the containers, the temporary folder and everything stored under `org.fuchss.atrium.demo`, including its Keychain item. It needs Docker, ImageMagick (`magick`), free ports 8008 and 8080, and Screen Recording permission for the terminal.
