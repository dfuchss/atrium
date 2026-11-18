# Service definitions (expert mode)

Every service is defined by a TOML file: the [built-in ones](Services) are compiled into Atrium from [`services/`](https://github.com/<<REPO>>/tree/main/services), and your own live in the `services/` folder of Atrium's config folder.

## Where the files live

| Platform | Folder |
|---|---|
| macOS | `~/Library/Application Support/org.fuchss.atrium/services/` |
| Linux | `~/.config/org.fuchss.atrium/services/` |
| Windows | `%APPDATA%\org.fuchss.atrium\services\` |

Settings → Custom services → *Open folder* opens it (Settings → About shows the path too). If the folder holds no `*.toml` file and no `README.md`, *Open folder* writes a short `README.md` and `example.toml.disabled`, a fully commented definition to copy from.

## How definitions are loaded

- Atrium reads every `*.toml` file of the folder at startup and on *Reload*, in file name order. Other files, such as `*.toml.disabled`, are ignored.
- A definition whose `id` equals a built-in service (`element`, `slack`, `teams`, `discord`, `whatsapp`, `rainbow`, `linkedin`, `mastodon`, `bluesky`) replaces it; a new `id` adds a service to the *Add account* list. If two files define the same `id`, the later file name wins.
- Custom services created in Settings → Custom services are stored in `accounts.json`, not as files. Their ids start with `custom-`. If a file defines the same id as a custom service, the custom service is used and the log says so.
- On *Reload*, accounts whose view would change with the new definition reload their page (the login stays); the others only get the new link and permission rules.

## Errors

- An invalid file is skipped. Settings → Custom services shows the error, and the log has it as `invalid service definition: …`; the startup line of the log counts them (`… service errors`).
- While any file fails, the services that only a failed file could have defined keep their previous definition, so a typo does not take a service away from its accounts.
- The in-app editor checks the text with the loader's own rules while you type and shows the first problem, with line and column where the TOML parser knows them. It refuses to save an invalid file.
- Checked when a file is loaded: the TOML syntax and types, that `id` and `name` are not empty, the `color`, and that the regular expressions compile. The `url` is checked when an account is loaded: it must be an `http(s)` URL once `{instance}` is replaced, and an account without an instance value (and without `instance_default`) cannot load.
- Unknown keys are ignored without an error, so a misspelled key has no effect.

## Minimal example

```toml
id = "mastodon"
name = "Mastodon"
color = "#6364FF"
url = "https://{instance}/"
instance_label = "Instance"
instance_default = "mastodon.social"
user_agent = "webkit"
internal_hosts = ["{instance}"]

[unread]
title_regex = '^\((\d+)\)'
```

Only `id`, `name`, `color` and `url` are required. Without an `[unread]` rule, a service from a file shows no unread count.

## Full example

Based on `services/element.toml` and `services/discord.toml`:

```toml
id = "element"                          # required, unique; never change it once accounts use it
name = "Element"                        # required: "Add account" list and default account label
color = "#0DBD8B"                       # required: sidebar colour
url = "https://{instance}/"             # required: start page, {instance} = the account's value
instance_label = "Element web host"     # ask each account for {instance}, with this label
instance_default = "app.element.io"     # used when the account leaves it empty
user_agent = "webkit"                   # "webkit" | "chrome" | "default" | a literal UA string
internal_hosts = ["{instance}", "*.element.io", "*.matrix.org"]
allow_media = true                      # camera, microphone and screen capture by default
# Element's single-instance session lock: always stale at page start in Atrium.
clear_local_storage = ["react_sdk_session_lock_ping", "react_sdk_session_lock_owner", "react_sdk_session_lock_claimant"]

[unread]
# Evaluated every second; a value wins over the title rules.
poll = """(() => {
  const home = document.querySelector('.mx_SpaceButton');
  if (!home) return;                    // no value yet: the title rules apply
  const count = parseInt(home.querySelector('.mx_SpacePanel_notificationBadge')?.textContent) || 0;
  return { direct: count, indirect: false };
})()"""
title_regex = '\[(\d+)\]'               # "Element [3]": group 1 is the count
title_indirect_regex = '\*'             # a match shows a dot
```

The `poll` here is shortened; the real one in `services/element.toml` also counts the badges of spaces.

## Keys

### Top level

| Key | Type | Required | Default | What it does |
|---|---|---|---|---|
| `id` | string | yes | | Unique id of the service; accounts refer to it. Equal to a built-in id, the file replaces that service. Must not be empty. |
| `name` | string | yes | | Shown in the *Add account* list, as default account label and in notification titles. Must not be empty. |
| `color` | string | yes | | Sidebar colour: `#` and 3 to 8 hex digits (`#rgb`, `#rrggbb`, also with alpha). |
| `url` | string | yes | | Start page. `{instance}` is replaced by the account's instance value. Must be `http` or `https`. |
| `instance_label` | string | no | none | Makes each account ask for an instance value, with this label (e.g. the server host or workspace). |
| `instance_default` | string | no | none | Instance value of accounts that leave it empty. |
| `user_agent` | string | no | `"default"` | The user agent the pages see, see [User agent](#user-agent). Accounts can override it. |
| `internal_hosts` | array of strings | no | `[]` | The service's own hosts, see [Hosts](#hosts). |
| `allow_media` | boolean | no | `false` | Grant camera, microphone and screen capture by default. Accounts can override it (*Camera & microphone*). |
| `clear_local_storage` | array of strings | no | `[]` | `localStorage` keys removed at the start of every page, before the page's own scripts run, e.g. single-instance session locks left behind when Atrium was quit without unloading the page (an account has only one page, so such a lock is always stale). |
| `[unread]` | table | no | no rules | How unread messages are detected, see below. |

### `[unread]`

| Key | Type | Default | What it does |
|---|---|---|---|
| `title_regex` | string (regular expression) | none | Applied to the page title; capture group 1 is the direct unread count (mentions, direct messages). A title that does not match, or a group 1 that is no number, counts 0. |
| `title_indirect_regex` | string (regular expression) | none | Applied to the page title; a match shows the indirect unread dot. |
| `poll` | string (JavaScript expression) | none | Evaluated in the page, see [Poll](#poll). |

The regular expressions use the syntax of Rust's [`regex`](https://docs.rs/regex/latest/regex/#syntax) crate (no look-around or back-references). In TOML, write them in single quotes (`'^\((\d+)\)'`), which keep backslashes as they are.

### Poll

- Atrium evaluates the expression every second in each loaded account's page (not in sleeping or disabled accounts), in the page's main frame and with the page's own rights. A definition file from someone else can read everything the page shows: only use expressions you understand.
- It must return, synchronously, a number (the direct unread count) or an object `{ direct: <number>, indirect: <boolean> }`. Negative numbers and `NaN` count 0, fractions are cut off, and huge values are capped. A `Promise` is not awaited: it arrives as an empty object and counts 0.
- `undefined`, `null` or any other value means *no value*: the title rules apply again. Return nothing while the page has not rendered what you count.
- An exception or a syntax error in the expression counts as no value too, and so does a value that cannot be turned into JSON. A poll result larger than 256 KB is dropped and logged once (`poll result for …`), until a poll succeeds again.
- While the poll yields a value, it wins over `title_regex` and `title_indirect_regex`. A service with both uses the title rules as a fallback.
- There is no timeout: an expression that blocks, blocks the page. Keep it short; it runs every second.

### User agent

| Value | User agent |
|---|---|
| `"webkit"` | A desktop Safari of the platform Atrium runs on, e.g. for sites that reject the web view's bare user agent. Every built-in service uses it. |
| `"chrome"` | A desktop Chrome of the platform Atrium runs on, for sites that only accept Chromium-based browsers. |
| `"default"` or no key | The web view's own user agent. |
| anything else | Used literally as the `User-Agent`. |

On Windows, new accounts of a service that asks for `"webkit"` present `"chrome"` instead, as WebView2 is Chromium. Each account can choose *Service default*, *WebKit browser*, *Chrome* or a custom user agent; changing it reloads the account.

### Hosts

`internal_hosts` lists the service's own hosts: an exact host, or `*.domain`, which matches the domain itself and all its subdomains (case-insensitive). `{instance}` is replaced by the host part of the account's instance value; an entry with `{instance}` is dropped for an account without one.

The list scopes:
- **camera, microphone and screen capture**: granted only while the account's page (or its sign-in or app window) shows one of these hosts, and only if media are allowed. List every host a call runs on (e.g. a separate meeting domain);
- **site icons**: fetched only from the page's own host or one of these hosts;
- the window titles and the log.

Where links go is not part of the definition but an account setting: *Open links in* (system browser or app window, or the global setting in Settings → Links) and *Always open in the app*, a list of hosts whose new windows always open inside the app with the account's login. Sign-in popups always stay in the app.

### The instance value

Each account of a service with `instance_label` asks for its instance value; it is trimmed, and a pasted `https://` and trailing slashes are removed. It may hold a path (`chat.example.org/element`), which `url` keeps; `internal_hosts` only use the host part. Changing it in the account's settings reloads the account.

## Checking a title rule

To see the titles a service really shows, start a debug build with `ATRIUM_LOG_TITLES=1` (see [Debug options](Debug-Options)): each unread change is logged with the title.

## Demo files

The settings UI also runs in a plain browser as a demo, without Atrium's backend (see [Debug options](Debug-Options#browser-demo)). Its services folder lives in memory with three files: `wiki.toml` (a valid definition), `broken.toml` (a syntax error, to show the editor's error display) and `example.toml.disabled`. The demo checks them line by line only; the real checks are those described above.
