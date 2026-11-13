# Service definitions

Every `*.toml` file in this folder defines one messenger service (same format as the
built-in services). Files are read at startup and when you click "Reload" in
Settings > Custom services; no restart needed.

- A file whose `id` equals a built-in service (`slack`, `element`, ...) overrides it.
- A file with a new `id` adds a service to the "Add account" list.
- Files that do not end in `.toml` (such as `example.toml.disabled`) are ignored.
- Invalid files are skipped; the error is shown in Settings and written to the log. The
  previous definition of that service stays active.
- Accounts of a changed service reload their page on Reload (the login stays).

Start from `example.toml.disabled`: copy it to `myservice.toml` and edit it. Every field
is documented there.

Required fields: `id`, `name`, `color`, `url`.

Per-account part of the URL: put `{instance}` in `url` (and `internal_hosts`), e.g.
`url = "https://{instance}/"` for a self-hosted server or `https://{instance}.slack.com/`
for a workspace. Each account then asks for it; `instance_label` names the field and
`instance_default` pre-fills it.

`internal_hosts` also scopes camera, microphone and screen capture: they are granted only
while the account's page (or its sign-in/app window) shows one of these hosts, and only if
the account allows media. List every host a call runs on (e.g. a separate meeting domain).

`poll` is JavaScript that runs inside the service page with the page's own rights, every
second while the account is loaded. Only use expressions you understand; a service file
from someone else can read everything the page shows.
