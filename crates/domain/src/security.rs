//! URL policies shared by the core and the adapters (pure functions).

use protocol::LinkTarget;

use crate::ViewPolicy;

/// Schemes that may be handed to the OS opener (browser / mail client). Everything else
/// (`file:`, custom app schemes such as `x-apple.systempreferences:`, `javascript:`...) is
/// dropped, so a page cannot launch local files or other apps through us.
pub(crate) fn opener_url_allowed(url: &str) -> bool {
    url::Url::parse(url.trim()).is_ok_and(|parsed| match parsed.scheme() {
        "http" | "https" => parsed.host_str().is_some(),
        "mailto" => true,
        _ => false,
    })
}

/// Hosts under which Tauri serves Atrium's own UI and IPC on Windows, where its schemes are
/// mapped to `http://tauri.localhost` and `http://ipc.localhost` in every webview.
const APP_UI_HOSTS: [&str; 2] = ["tauri.localhost", "ipc.localhost"];

/// Navigation allow-list of service views and their popup/app windows: web content only.
/// Denies `file:`, `tauri:`, `ipc:`, `asset:` and every other custom scheme, and the hosts of
/// Atrium's own UI, so a page cannot load the UI and reach its commands. macOS and Linux check
/// iframes too; `WebView2` reports only main-frame navigations.
pub fn nav_allowed(url: &url::Url) -> bool {
    match url.scheme() {
        "https" | "http" => !url
            .host_str()
            .is_some_and(|host| APP_UI_HOSTS.contains(&host)),
        "about" | "blob" | "data" => true,
        _ => false,
    }
}

/// What a new-window request may become before the account's link policy decides.
#[derive(Debug, PartialEq, Eq)]
pub enum NewWindowGate {
    /// `mailto:` goes to the core as an external link (mail app), as in the navigation hook.
    MailApp,
    /// Not a page a window may load ([`nav_allowed`]), e.g. Atrium's own UI hosts.
    Denied,
    Open,
}

pub fn new_window_gate(url: &url::Url) -> NewWindowGate {
    if url.scheme() == "mailto" {
        NewWindowGate::MailApp
    } else if nav_allowed(url) {
        NewWindowGate::Open
    } else {
        NewWindowGate::Denied
    }
}

/// Where a new-window request that passed the [`NewWindowGate`] goes.
#[derive(Debug, PartialEq, Eq)]
pub enum NewWindowRoute<S> {
    /// An in-app window of the requested size `S`.
    SignInPopup(S),
    /// An in-app window, as the URL's host is one of the account's app hosts.
    AppHost,
    /// An in-app window, as the account's links open in one ([`LinkTarget::AppWindow`]).
    AppWindow,
    /// The system browser ([`LinkTarget::SystemBrowser`]); where the engine tells a requested
    /// size only once the window is ready to show (Linux), the adapter may still find a sign-in
    /// popup.
    SystemBrowser,
}

/// The route of a request for `url`, with the `size` the page asked for, under `policy`: a
/// requested size makes a sign-in popup, an app host an app window, anything else follows the
/// account's link target.
pub fn new_window_route<S>(
    url: &url::Url,
    size: Option<S>,
    policy: &ViewPolicy,
) -> NewWindowRoute<S> {
    if let Some(size) = size {
        return NewWindowRoute::SignInPopup(size);
    }
    if url.host_str().is_some_and(|host| policy.is_app_host(host)) {
        return NewWindowRoute::AppHost;
    }
    match policy.link_target {
        LinkTarget::SystemBrowser => NewWindowRoute::SystemBrowser,
        LinkTarget::AppWindow => NewWindowRoute::AppWindow,
    }
}

/// Longest URL path kept by [`redact_url`], in characters.
const REDACTED_PATH_MAX_CHARS: usize = 200;

/// URL as written to the log: `scheme://host[:port]/path`, without user info, query and
/// fragment (they carry tokens, search terms and message ids). URLs without a host (`mailto:`,
/// `data:`, `blob:`) keep only their scheme; unparsable input is not echoed at all.
pub fn redact_url(raw: &str) -> String {
    let Ok(url) = url::Url::parse(raw.trim()) else {
        return "<invalid URL>".into();
    };
    let Some(host) = url.host_str() else {
        return format!("{}:", url.scheme());
    };
    let port = url
        .port()
        .map_or_else(String::new, |port| format!(":{port}"));
    let path: String = url.path().chars().take(REDACTED_PATH_MAX_CHARS).collect();
    format!("{}://{host}{port}{path}", url.scheme())
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
            "http://tauri.localhost/index.html?dialog=accounts",
            "https://tauri.localhost/",
            "http://IPC.localhost/cmd",
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
    fn new_windows_send_mail_links_to_the_mail_app_and_deny_the_app_ui() {
        let gate = |url: &str| new_window_gate(&url::Url::parse(url).unwrap());
        assert_eq!(gate("mailto:a@b.org"), NewWindowGate::MailApp);
        assert_eq!(gate("https://example.org/"), NewWindowGate::Open);
        assert_eq!(gate("about:blank"), NewWindowGate::Open);
        assert_eq!(gate("http://tauri.localhost/"), NewWindowGate::Denied);
        assert_eq!(gate("https://ipc.localhost/x"), NewWindowGate::Denied);
        assert_eq!(gate("file:///C:/Windows/win.ini"), NewWindowGate::Denied);
        assert_eq!(gate("javascript:alert(1)"), NewWindowGate::Denied);
    }

    #[test]
    fn new_windows_go_by_size_then_app_host_then_link_target() {
        let policy = |link_target| ViewPolicy {
            link_target,
            allow_media: false,
            app_hosts: vec!["*.sso.example".into()],
        };
        let route = |url: &str, size: Option<(u32, u32)>, link_target| {
            new_window_route(&url::Url::parse(url).unwrap(), size, &policy(link_target))
        };
        let (link, app_host) = ("https://example.org/", "https://login.sso.example/");
        for link_target in [LinkTarget::SystemBrowser, LinkTarget::AppWindow] {
            assert_eq!(
                route(link, Some((500, 600)), link_target),
                NewWindowRoute::SignInPopup((500, 600))
            );
            assert_eq!(route(app_host, None, link_target), NewWindowRoute::AppHost);
        }
        assert_eq!(
            route(link, None, LinkTarget::SystemBrowser),
            NewWindowRoute::SystemBrowser
        );
        assert_eq!(
            route(link, None, LinkTarget::AppWindow),
            NewWindowRoute::AppWindow
        );
        assert_eq!(
            route("about:blank", None, LinkTarget::SystemBrowser),
            NewWindowRoute::SystemBrowser
        );
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

    #[test]
    fn redacted_urls_keep_only_scheme_host_and_path() {
        assert_eq!(
            redact_url("https://user:pw@app.slack.com:8443/client/T1?token=abc#msg-9"),
            "https://app.slack.com:8443/client/T1"
        );
        assert_eq!(redact_url("https://x.org"), "https://x.org/");
        assert_eq!(redact_url("mailto:alice@example.org?subject=hi"), "mailto:");
        assert_eq!(redact_url("data:text/html,secret"), "data:");
        assert_eq!(redact_url("blob:https://x.org/1"), "blob:");
        assert_eq!(redact_url("not a url ?q=secret"), "<invalid URL>");
        let long = format!("https://x.org/{}", "a".repeat(500));
        assert_eq!(redact_url(&long).len(), "https://x.org".len() + 200);
    }
}
