# Built-in services

The built-in services are defined in [`services/`](https://github.com/<<REPO>>/tree/main/services), in the format of [service definitions](Service-Definitions); a definition of your own with the same id overrides one.

| Service | Opens | Unread count | Camera and microphone |
|---|---|---|---|
| Element / Matrix | Any Element web host, asked per account (default `app.element.io`) | Element's own badges in the space panel (messages); title `Element [n]` (rooms) as fallback | Yes |
| Slack | `app.slack.com/client` | Title `(n)`; a leading `*` shows a dot | Yes |
| Microsoft Teams | `teams.microsoft.com/v2/` | Title `(n)` | Yes |
| Discord | `discord.com/app` | Title `(n)` for mentions; a leading `•` (unread channels) shows a dot | Yes |
| WhatsApp | `web.whatsapp.com` | Title `(n)`: unread chats | Yes |
| Rainbow | `web.openrainbow.com` | Title `(n)` | Yes |
| LinkedIn | Its messaging page | Title `(n)` | No |
| Mastodon | Any instance, asked per account (default `mastodon.social`) | Title `(n)` | No |
| Bluesky | `bsky.app` | Title `(n)` | No |

"Title `(n)`" means a page title that starts with the count in brackets, e.g. `(3) WhatsApp`. "Camera and microphone" is the service's default; each account can change it, and it is granted only while the page shows one of the service's own hosts.

Every built-in service presents the user agent of a desktop WebKit browser (the `webkit` preset); on Windows, new accounts present Chrome's instead (see [Windows](Windows)). The user agent can be changed per account.

## Notes per service

- **Element:** the Home button's badge counts messages, while the title counts rooms. With *Show all rooms in Home* off, Home counts only rooms outside any space; Atrium then adds the badges of the top-level spaces you joined, and the count is at least the title's room count. Dots on Home or a space show as an indirect unread dot. At page start, Atrium removes Element's single-instance session lock from the page's storage: an account has only one Element page, so a lock found then is stale, left behind when Atrium was quit without unloading the page.
- **Microsoft Teams** rejects the web view's default user agent and needs the one of a desktop browser. Its own hosts also cover the Microsoft sign-in, Office, SharePoint and Live hosts.
- **Bluesky, Slack and Teams:** the title formats are taken from the sites and not yet verified in every case; if the count stays empty, a debug build logs the titles with `ATRIUM_LOG_TITLES` (see [Debug options](Debug-Options)).

Anything else can be added as a custom service in Settings → Custom services: a name, a URL, and optionally a title pattern for unread counts; without one, a title that starts with `(n)` counts.
