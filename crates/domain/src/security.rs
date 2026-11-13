//! URL policies shared by the core and the adapters (pure functions).

/// Schemes that may be handed to the OS opener (browser / mail client). Everything else
/// (`file:`, custom app schemes such as `x-apple.systempreferences:`, `javascript:`...) is
/// dropped, so a page cannot launch local files or other apps through us.
pub fn opener_url_allowed(url: &str) -> bool {
    url::Url::parse(url.trim()).is_ok_and(|parsed| match parsed.scheme() {
        "http" | "https" => parsed.host_str().is_some(),
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

/// Longest page title shown in a window title, in characters.
const WINDOW_TITLE_MAX_CHARS: usize = 120;

/// Title of a popup/app window: `"<page title> — <host>"`, so a page cannot pose as another
/// site through its own title. `own_host` = the host belongs to the account (internal or app
/// host); pages elsewhere get an `"External: "` prefix. Host-less pages show their scheme.
pub fn window_title(page_title: &str, page: &url::Url, own_host: bool) -> String {
    let origin = page
        .host_str()
        .map_or_else(|| format!("{}:", page.scheme()), str::to_owned);
    let page_title: String = page_title
        .trim()
        .chars()
        .take(WINDOW_TITLE_MAX_CHARS)
        .collect();
    let prefix = if own_host { "" } else { "External: " };
    if page_title.is_empty() {
        format!("{prefix}{origin}")
    } else {
        format!("{prefix}{page_title} — {origin}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opener_schemes() {
        for ok in [
            "https://x.org/a?b",
            "http://localhost:8080/",
            "mailto:a@b.org",
            " https://x.org ",
        ] {
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
        for ok in [
            "https://app.slack.com/",
            "http://x.org",
            "about:blank",
            "about:srcdoc",
            "blob:https://x.org/1",
            "data:text/html,x",
        ] {
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

    #[test]
    fn window_titles_name_the_host() {
        let page = |raw: &str| url::Url::parse(raw).unwrap();
        assert_eq!(
            window_title("Sign in", &page("https://login.slack.com/x?y"), true),
            "Sign in — login.slack.com"
        );
        assert_eq!(
            window_title(" Google Accounts ", &page("https://evil.example/"), false),
            "External: Google Accounts — evil.example"
        );
        assert_eq!(window_title("", &page("https://a.org/"), true), "a.org");
        assert_eq!(window_title("", &page("about:blank"), true), "about:");
        let long = "x".repeat(500);
        let title = window_title(&long, &page("https://a.org/"), true);
        assert!(title.ends_with(" — a.org") && title.chars().count() < 140);
    }
}
