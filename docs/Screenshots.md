# Screenshots

`scripts/screenshots.sh` takes the screenshots in `docs/screenshots/` (used by the README and this wiki) with demo data only, on macOS. It runs a throwaway Matrix homeserver (tuwunel) and Element web in Docker, seeds demo users and chats, builds a debug bundle with its own bundle id `org.fuchss.atrium.demo` into `target/screenshots`, and starts it with the [debug options](Debug-Options) and a temporary config folder.

The own bundle id keeps the demo's WebKit data, logs, caches and WebCrypto Keychain item apart from a regular installation (sharing `org.fuchss.atrium` would make macOS ask for access to its Keychain item). At the end it removes the containers, the temporary folder and everything stored under `org.fuchss.atrium.demo`, including its Keychain item.

It needs Docker, the Tauri CLI, Trunk, `python3`, ImageMagick (`magick`), free ports 8008 and 8080, and Screen Recording permission for the terminal.

| Variable | Effect |
|---|---|
| `ATRIUM_SHOTS_TARGET_DIR=<dir>` | Cargo target folder for the demo bundle (default `target/screenshots`, so a running debug bundle in `target/debug` is not replaced). |
| `ATRIUM_SHOTS_SKIP_BUILD=1` | Reuse the bundle built by an earlier run. |
| `ATRIUM_SHOTS_KEEP_SECONDS=<n>` | No screenshots: keep the demo running for `n` seconds, then print its unread log lines (checks the unread rules against a real Element). |
| `ATRIUM_SHOTS_WINDOW_POSITION=<x>,<y>` | Window position (default `24,48` on the main display). |
