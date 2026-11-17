# Debug options

Debug builds (`cargo tauri dev`, `cargo tauri build --debug`) read these environment variables; release builds ignore them. They let a second, isolated instance run next to the regular one, e.g. for screenshots. An empty value counts as not set.

| Variable | Effect |
|---|---|
| `ATRIUM_CONFIG_DIR=<dir>` | Config folder instead of the app's: `accounts.json`, `services/` and `atrium.log`. Such an instance never changes the login item, and on Linux and Windows it starts even while Atrium runs. On Windows it also needs its own identifier, see [below](#a-second-instance-on-windows). |
| `ATRIUM_ALWAYS_ON_TOP=1` | Keeps the main window on top. macOS does not paint occluded windows, so `screencapture -l` would capture a blank window. |
| `ATRIUM_WINDOW_POSITION=<x>,<y>` | Position of the main window in logical pixels (the main display starts at `0,0`), e.g. to open it on a Retina display. |
| `ATRIUM_DEV_INIT_SCRIPT=<file>` | JavaScript appended to the init script of every service view (the screenshot script seeds a demo Element session with it). Atrium stops if the file cannot be read. |
| `ATRIUM_LOG_TITLES=1` | Logs page titles with each unread change, to check a service's `title_regex` against what it really shows. Titles name chats and senders, so this is off otherwise. |
| `ATRIUM_DEV_RELEASE_DIR=<dir>` | Folder that stands in for the latest GitHub release, to try an update by hand: `release.json` (the GitHub latest-release JSON; asset URLs must start with `https://github.com/dfuchss/atrium/releases/download/`, only their file name is used), `SHA256SUMS` and the asset. The debug build then counts as installed, so it updates itself when it runs from an installable location. |
| `ATRIUM_DEV_UI_QUERY=<query>` | Query string for the sidebar UI. `dialog=<tab>` opens the settings dialog on a tab (`accounts`, `general`, `notifications`, `dock`, `links`, `custom`, `about`), `edit=<index>` opens the edit form of an account, `addcustom=1` the custom service form, `editservice=<file>` the definition editor (demo files `wiki.toml`, `broken.toml`). |

The variables of `scripts/screenshots.sh` are on [Screenshots](Screenshots).

## A second instance on Windows

Changing `%APPDATA%` or `%LOCALAPPDATA%` does not move Atrium's folders (see [Windows](Windows#data-locations)). A development instance therefore needs its own identifier besides its own config folder; afterwards delete `%APPDATA%\org.fuchss.atrium.dev` and `%LOCALAPPDATA%\org.fuchss.atrium.dev`:

```sh
cd app/src-tauri
cargo tauri build --debug --no-bundle --config '{"identifier":"org.fuchss.atrium.dev"}'
ATRIUM_CONFIG_DIR="$TEMP/atrium-dev" ../../target/debug/atrium.exe
```

Its notifications show Atrium's name only once its Start menu shortcut, with the development identifier, is installed (see [Windows](Windows#known-limitations)).

## Browser demo

The sidebar and settings UI (`app/ui`) also runs in a plain browser, without Atrium's backend: when the page finds no Tauri, or with `?demo` in its query string, it shows made-up accounts and an in-memory services folder (`wiki.toml`, `broken.toml`, `example.toml.disabled`). The query parameters of `ATRIUM_DEV_UI_QUERY` work there too, so the settings can be opened in a given state without clicks.
