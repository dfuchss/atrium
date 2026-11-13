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
