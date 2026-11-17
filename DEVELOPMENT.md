# Development

How to build and check Atrium quickly. The details are in the wiki: [Building](https://github.com/dfuchss/atrium/wiki/Building) (macOS, Linux, Windows), [Architecture](https://github.com/dfuchss/atrium/wiki/Architecture), [Checks](https://github.com/dfuchss/atrium/wiki/Checks), [Debug options](https://github.com/dfuchss/atrium/wiki/Debug-Options), [Screenshots](https://github.com/dfuchss/atrium/wiki/Screenshots) and [Releasing](https://github.com/dfuchss/atrium/wiki/Releasing).

## Requirements

- [Rust](https://rustup.rs/) (stable) with the `wasm32-unknown-unknown` target, [Trunk](https://trunkrs.dev/) and the [Tauri CLI](https://v2.tauri.app/reference/cli/).
- On Linux the WebKitGTK and GTK development packages, on Windows the Microsoft C++ Build Tools and Git Bash: see [Building](https://github.com/dfuchss/atrium/wiki/Building).

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk tauri-cli --locked
```

## Build and run

```sh
cd app/src-tauri
cargo tauri dev                          # run with live-reloading UI
cargo tauri build                        # macOS: Atrium.app, Linux: AppImage
cargo tauri build --no-bundle            # Windows: target/release/atrium.exe
```

## Check

```sh
scripts/check.sh                         # everything CI checks: fmt, UI build, clippy, tests
```

`scripts/check.sh` must pass before every commit. User-visible changes get an entry under `## [Unreleased]` in [`CHANGELOG.md`](CHANGELOG.md), in the format described in [Releasing](https://github.com/dfuchss/atrium/wiki/Releasing#changelog).

The wiki pages live in [`docs/`](docs) and are mirrored into the wiki on every push to `main`; edit them there.
