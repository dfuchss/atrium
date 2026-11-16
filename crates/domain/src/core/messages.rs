//! The messages exchanged with the outside world: [`Input`]s into the core and the
//! [`Effect`]s it asks the adapters to perform, with their payload types.

use protocol::{AppSettings, LinkTarget, Snapshot, Theme, UiCommand};
use uuid::Uuid;

use crate::catalog::ServiceDef;
use crate::model::Persisted;

/// Rectangle in logical pixels, relative to the window's content area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Everything an adapter needs to create one service view.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewSpec {
    pub account: Uuid,
    pub url: url::Url,
    /// `WKWebView` data store identifier (= account id bytes) -> isolated, persistent session.
    pub store_id: [u8; 16],
    pub user_agent: Option<String>,
    /// Injected at document start ([`crate::NOTIFY_SHIM`], plus the account's custom CSS).
    pub init_script: String,
    /// Hosts (`*.` wildcards) of the service itself, `{instance}` resolved. They scope media
    /// permission ([`ViewSpec::media_allowed_at`]); where a new window goes is decided by the
    /// account's [`LinkTarget`] (see [`Effect::SetViewPolicy`]).
    pub internal_hosts: Vec<String>,
}

impl ViewSpec {
    /// True for URLs whose host matches `internal_hosts`, and for non-http(s) URLs
    /// (about:, blob:, data:).
    pub fn is_internal(&self, url: &url::Url) -> bool {
        if !matches!(url.scheme(), "http" | "https") {
            return true;
        }
        match url.host_str() {
            None => true,
            Some(host) => crate::catalog::host_matches_any(&self.internal_hosts, host),
        }
    }

    /// Whether camera, microphone or screen capture may be granted while `page` is shown.
    /// The engine does not tell which origin asks, so the main-frame page must be one of the
    /// service's own http(s) hosts; anything else (unknown page, foreign site) is denied.
    pub fn media_allowed_at(&self, allow_media: bool, page: Option<&url::Url>) -> bool {
        allow_media
            && page.is_some_and(|page| {
                matches!(page.scheme(), "http" | "https")
                    && page.host_str().is_some_and(|host| {
                        crate::catalog::host_matches_any(&self.internal_hosts, host)
                    })
            })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NotificationReq {
    pub account: Uuid,
    /// Id assigned by the shim in the page, passed back on click; None for notifications of
    /// the app itself (finished downloads).
    pub shim_id: Option<u64>,
    /// Already prefixed with the account name, e.g. "Slack (KIT): Alice".
    pub title: String,
    /// The message text, or "New message" when previews are off.
    pub body: String,
    /// Play the default notification sound.
    pub sound: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockBadge {
    None,
    Count(u32),
    /// Only indirect unread somewhere.
    Dot,
}

/// Per-account decisions the adapter must take synchronously inside native callbacks
/// (new-window requests, permission requests), so it keeps the latest value per account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewPolicy {
    /// Where non-popup new windows go.
    pub link_target: LinkTarget,
    /// Grant camera/microphone/screen capture; otherwise such requests are denied.
    pub allow_media: bool,
    /// Hosts (exact or `*.domain`) whose new windows always open in the app.
    pub app_hosts: Vec<String>,
}

impl ViewPolicy {
    /// True if a new window to `host` must open in the app, whatever the link target.
    pub fn is_app_host(&self, host: &str) -> bool {
        crate::catalog::host_matches_any(&self.app_hosts, host)
    }
}

/// Native menu commands that act on the active account (or the app).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Reload,
    Back,
    Forward,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    OpenInBrowser,
    ToggleDnd,
}

/// One enabled account as shown in the app menu ("Accounts") and the tray menu.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuAccount {
    pub id: Uuid,
    pub label: String,
    pub unread: u32,
}

/// Content of the native menus; emitted ([`Effect::UpdateMenus`]) only when it changes.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuModel {
    /// Enabled accounts in sidebar order.
    pub accounts: Vec<MenuAccount>,
    pub dnd: bool,
    /// Whether the tray (menu bar) icon exists.
    pub show_tray: bool,
    /// Tray title: same rules as the dock badge ("3", "•", or empty).
    pub tray_title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// First input after the window exists; creates the views of all enabled accounts.
    Started {
        width: f64,
        height: f64,
    },
    Ui(UiCommand),
    /// Native app/tray menu command.
    Menu(MenuAction),
    /// Native title-changed hook of the account's view.
    TitleChanged {
        account: Uuid,
        title: String,
    },
    /// Raw string handed to the `eval_with_callback` callback for a [`Effect::Poll`] script
    /// (JSON encoding of the script's string result; the core decodes it).
    PollResult {
        account: Uuid,
        raw: String,
    },
    PageLoaded {
        account: Uuid,
        url: String,
    },
    /// The view tried to open a new window and the account's link target is the system browser
    /// (already denied by the adapter).
    ExternalLink {
        account: Uuid,
        url: String,
    },
    NotificationClicked {
        account: Uuid,
        shim_id: Option<u64>,
    },
    /// A download of the account's view was saved (`file_name` in the Downloads folder).
    DownloadFinished {
        account: Uuid,
        file_name: String,
    },
    /// Window content size in logical pixels.
    WindowResized {
        width: f64,
        height: f64,
    },
    /// Every second: drives polling, hibernation and sleep detection. `now_ms` = wall clock
    /// (ms since the Unix epoch).
    Tick {
        now_ms: u64,
    },
    /// The displays woke, so the user is back; a dark wake (Power Nap) never wakes them.
    /// `now_ms` as for [`Input::Tick`].
    ScreensWoke {
        now_ms: u64,
    },
    /// Callback of [`Effect::FindIcon`]: JSON encoding of the icon URL (or null), and the
    /// page shown when it was found (read by the adapter, not reported by the page).
    IconFound {
        account: Uuid,
        raw: String,
        page_url: Option<String>,
    },
    /// Result of [`Effect::FetchIcon`]: the image as `data:` URL, None on failure.
    IconFetched {
        account: Uuid,
        data_url: Option<String>,
    },
    /// Result of [`Effect::FetchSimpleIcon`] for `slug`.
    SimpleIconFetched {
        account: Uuid,
        slug: String,
        result: SimpleIconResult,
    },
    /// Result of [`Effect::LoadServices`]: the valid definitions of `<config>/services/*.toml`
    /// (in file order) and a message per file that could not be read or parsed.
    ServicesLoaded {
        defs: Vec<ServiceDef>,
        errors: Vec<String>,
    },
}

/// Outcome of a Simple Icons download.
#[derive(Debug, Clone, PartialEq)]
pub enum SimpleIconResult {
    /// The white SVG glyph as `data:image/svg+xml;base64,...` URL.
    Found(String),
    /// Simple Icons has no icon with this slug (HTTP 404).
    NotFound,
    /// Any other failure (network, size, content type).
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    CreateView {
        spec: ViewSpec,
        bounds: Bounds,
        visible: bool,
    },
    SetBounds {
        account: Uuid,
        bounds: Bounds,
    },
    /// Bounds of the sidebar (`ui`) webview.
    SetUiBounds(Bounds),
    Show(Uuid),
    Hide(Uuid),
    Destroy(Uuid),
    /// Delete cookies/storage of an account without a view (removed or unloaded).
    DeleteProfile {
        store_id: [u8; 16],
    },
    /// Delete cookies/storage/cache of a loaded view, then reload it.
    ClearData(Uuid),
    Reload(Uuid),
    /// Link target and media permission of this account's views (incl. its popups); the
    /// adapter decides synchronously from it. Emitted before every [`Effect::CreateView`] and
    /// whenever the effective policy changes (no view recreation needed).
    SetViewPolicy {
        account: Uuid,
        policy: ViewPolicy,
    },
    /// Page zoom of the view (1.0 = 100 %). Emitted after every create and on change.
    SetZoom {
        account: Uuid,
        zoom: f64,
    },
    /// Evaluate `script` and feed the result back as [`Input::PollResult`].
    Poll {
        account: Uuid,
        script: String,
    },
    /// Fire-and-forget script.
    Eval {
        account: Uuid,
        script: String,
    },
    /// Evaluate `script` (returns the best icon URL) and feed the result back as
    /// [`Input::IconFound`].
    FindIcon {
        account: Uuid,
        script: String,
    },
    /// Download the icon in the background and feed [`Input::IconFetched`].
    FetchIcon {
        account: Uuid,
        url: String,
    },
    /// Download the Simple Icons glyph `slug` and feed [`Input::SimpleIconFetched`].
    FetchSimpleIcon {
        account: Uuid,
        slug: String,
    },
    /// Open the URL currently shown in the account's view in `browser` (or the default).
    OpenInBrowser {
        account: Uuid,
        browser: Option<String>,
    },
    Notify(NotificationReq),
    SetDockBadge(DockBadge),
    /// Rebuild/update the app menu and tray from the model.
    UpdateMenus(MenuModel),
    /// Open `url` in `browser` (application name, e.g. "Firefox"), or the system default.
    /// Only http(s)/mailto URLs (see [`crate::security::opener_url_allowed`]).
    OpenExternal {
        url: String,
        browser: Option<String>,
    },
    FocusWindow,
    /// Create `<config>/services/` (with README and example when empty) and show it.
    OpenServicesFolder,
    /// Re-read the user's service files; answered with [`Input::ServicesLoaded`].
    LoadServices,
    /// Apply OS-level settings (launch at login, ...). Emitted on start and on every change.
    ApplySystemSettings(AppSettings),
    /// Set the app-wide appearance. Emitted on start and when the setting changes.
    ApplyTheme(Theme),
    Persist(Persisted),
    EmitSnapshot(Snapshot),
    /// Something worth logging (bad poll JSON, unknown account, ...).
    Log(String),
}
