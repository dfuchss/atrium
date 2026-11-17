# Privacy and security

- **Isolation:** every service view runs in its own web content process with its own data store: sandboxed by WebKit on macOS, without WebKit's sandbox on [Linux](Linux#data-locations), and on [Windows](Windows#data-locations) with a WebView2 browser process per account. Service pages get **no access** to Atrium's internal API.
- **Minimal script injection:** Atrium injects only a small script that forwards web notifications and routes new-window links, plus optional custom CSS. Unread counts are read from the page title in Rust.
- **Links:** only `http(s)` and `mailto` links are handed to the system, and in-page navigation is limited to web schemes. Pages can open windows or your browser only after a click (not on [Windows](Windows#known-limitations)) and only for the account you are looking at; external links are rate-limited, and the number of in-app windows per account is capped and rate-limited.
- **Camera and microphone** are available only to the service's own sites (its `internal_hosts`) and only if the account allows them.
- **Notifications** are capped per account, so no page can flood you.
- **Downloads** never replace an existing file (a taken name gets a number, like `report (1).pdf`); hidden files are refused. On macOS, downloads are quarantined.
- **Site icons** are fetched only over https from the page's or the service's own hosts, and never from your own computer or local network, even when a host name points there.
- **Stored data:** the account list is readable by you only; removing an account deletes its stored web data (retried at the next start if needed). If the saved accounts can't be read, Atrium starts read-only and leaves them untouched.
- **Private log:** URLs are logged without query and fragment, page titles not at all. The log is rotated to `atrium.log.1` at 5 MB and, on macOS and Linux, readable by you only. Where it lies: [macOS](macOS#data-locations), [Linux](Linux#data-locations), [Windows](Windows#data-locations).
- **Local only:** the settings UI runs under a strict Content Security Policy. Atrium has no telemetry and no server; everything stays on your computer. The update check asks the GitHub API for the latest release (with the Atrium version in the user agent); it can be switched off.
