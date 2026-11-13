//! URL policies shared by the core and the adapters (pure functions).

/// Schemes that may be handed to the OS opener (browser / mail client). Everything else
/// (`file:`, custom app schemes such as `x-apple.systempreferences:`, `javascript:`...) is
/// dropped, so a page cannot launch local files or other apps through us.
pub fn opener_url_allowed(url: &str) -> bool {
    url::Url::parse(url.trim()).is_ok_and(|u| match u.scheme() {
        "http" | "https" => u.host_str().is_some(),
        "mailto" => true,
        _ => false,
    })
}

/// Navigation allow-list of service views and their popup/app windows (main frame and
/// iframes alike): web content only. Denies `file:`, `tauri:`, `ipc:`, `asset:` and every
/// other custom scheme.
pub fn nav_allowed(url: &url::Url) -> bool {
    matches!(url.scheme(), "https" | "http" | "about" | "blob" | "data")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opener_schemes() {
        for ok in ["https://x.org/a?b", "http://localhost:8080/", "mailto:a@b.org", " https://x.org "] {
            assert!(opener_url_allowed(ok), "{ok}");
        }
        for bad in [
            "file:///etc/passwd",
            "x-apple.systempreferences:com.apple.preference.security",
            "javascript:alert(1)",
            "tauri://localhost/",
            "slack://open",
            "ftp://x.org/",
            "data:text/html,hi",
            "https://",
            "not a url",
            "",
        ] {
            assert!(!opener_url_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn navigation_schemes() {
        let u = |s: &str| url::Url::parse(s).unwrap();
        for ok in ["https://app.slack.com/", "http://x.org", "about:blank", "about:srcdoc", "blob:https://x.org/1", "data:text/html,x"] {
            assert!(nav_allowed(&u(ok)), "{ok}");
        }
        for bad in [
            "file:///Users/x",
            "tauri://localhost/index.html",
            "ipc://localhost/cmd",
            "asset://localhost/x",
            "http2://x",
            "x-apple.systempreferences:a",
            "javascript:alert(1)",
            "ws://x.org/",
            "mailto:a@b.org",
        ] {
            assert!(!nav_allowed(&u(bad)), "{bad}");
        }
    }
}
