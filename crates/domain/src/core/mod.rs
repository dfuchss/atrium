//! The reducer: `Core::handle(Input) -> Vec<Effect>`.

use std::collections::{HashMap, HashSet};

use protocol::{
    AccountSettings, AccountView, AppInfo, AppSettings, CustomService, DockBadgeMode, LinkTarget,
    ServiceInfo, Snapshot, UiCommand,
};
use regex::Regex;
use uuid::Uuid;

use crate::catalog::{Catalog, ServiceDef};
use crate::model::{Account, Persisted};

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
    /// WKWebView data store identifier (= account id bytes) -> isolated, persistent session.
    pub store_id: [u8; 16],
    pub user_agent: Option<String>,
    /// Injected at document start ([`crate::NOTIFY_SHIM`], plus the account's custom CSS).
    pub init_script: String,
    /// Hosts (`*.` wildcards) of the service itself. Only used for diagnostics: where a new
    /// window goes is decided by the account's [`LinkTarget`] (see [`Effect::SetViewPolicy`]).
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct NotificationReq {
    pub account: Uuid,
    /// Id assigned by the shim in the page; passed back on click.
    pub shim_id: u64,
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
        shim_id: u64,
    },
    /// Window content size in logical pixels.
    WindowResized {
        width: f64,
        height: f64,
    },
    /// Every second: drives polling, hibernation and wake detection. `now_ms` = wall clock
    /// (ms since the Unix epoch).
    Tick {
        now_ms: u64,
    },
    /// Callback of [`Effect::FindIcon`]: JSON encoding of the icon URL (or null).
    IconFound {
        account: Uuid,
        raw: String,
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

/// URL of the white glyph of a Simple Icons slug.
pub fn simple_icon_url(slug: &str) -> String {
    format!("https://cdn.simpleicons.org/{slug}/white")
}

/// Normalises a Simple Icons slug: trimmed, lowercase, `[a-z0-9]` only. `Ok(None)` = empty.
pub fn normalize_slug(raw: &str) -> Result<Option<String>, String> {
    let s = raw.trim().to_lowercase();
    if s.is_empty() {
        Ok(None)
    } else if s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        Ok(Some(s))
    } else {
        Err(format!(
            "invalid Simple Icons slug '{}' (only a-z and 0-9)",
            raw.trim()
        ))
    }
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
    Persist(Persisted),
    EmitSnapshot(Snapshot),
    /// Something worth logging (bad poll JSON, unknown account, ...).
    Log(String),
}

/// Unread state of one account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unread {
    pub direct: u32,
    pub indirect: bool,
}

/// Zoom levels of "Zoom In" / "Zoom Out".
pub const ZOOM_STEPS: [f64; 10] = [0.5, 0.67, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];
const ZOOM_MIN: f64 = 0.25;
const ZOOM_MAX: f64 = 5.0;
/// A gap between two ticks longer than this means the Mac was asleep.
pub const WAKE_GAP_MS: u64 = 30_000;

/// Returns the best icon URL of the page as an absolute URL: apple-touch-icon, else the
/// largest `link[rel~=icon]` by `sizes`, else `/favicon.ico`.
/// Picks the best site icon: an SVG `rel=icon` if present, otherwise the largest raster icon
/// (from `sizes` or a `NNxNN` in the file name) among `rel=icon` and apple-touch icons, falling
/// back to `/favicon.ico`. Returns an absolute URL (or null).
pub const ICON_SCRIPT: &str = r#"(()=>{try{const abs=h=>new URL(h,document.baseURI).href;let best=null,score=-1;for(const l of document.querySelectorAll('link[rel~="icon"][href],link[rel~="apple-touch-icon"][href],link[rel~="apple-touch-icon-precomposed"][href]')){const href=l.getAttribute('href');const rel=(l.getAttribute('rel')||'').toLowerCase();const touch=rel.includes('apple-touch');const z=(l.getAttribute('sizes')||'').toLowerCase();const svg=!touch&&((l.getAttribute('type')||'').includes('svg')||/\.svg(\?|$)/i.test(href)||z==='any');let s=0;if(svg)s=1e6;else{for(const p of z.split(/\s+/)){const n=parseInt(p,10);if(n>s)s=n;}if(!s){const m=href.match(/(\d{2,4})x\d{2,4}/);s=m?parseInt(m[1],10):(touch?180:16);}}if(!touch)s+=0.5;if(s>score){score=s;best=href;}}if(best)return abs(best);return location.origin+'/favicon.ico';}catch(e){return null;}})()"#;

/// Favicon lookups per account and app run.
const MAX_ICON_ATTEMPTS: u8 = 3;

pub struct Core {
    /// Built-in + user TOML services; `catalog` = this + custom services.
    base_catalog: Catalog,
    /// The user's TOML definitions applied on top of `builtin` in `base_catalog`.
    user_defs: Vec<ServiceDef>,
    /// Problems of the last load of the user's service files.
    service_errors: Vec<String>,
    /// The catalog without user files (built-ins only), the base of every reload.
    builtin: Catalog,
    catalog: Catalog,
    persisted: Persisted,
    id_gen: Box<dyn FnMut() -> Uuid + Send>,
    active: Option<Uuid>,
    dialog_open: bool,
    window: (f64, f64),
    started: bool,
    unread: HashMap<Uuid, Unread>,
    /// Last unread derived from the window title (fallback when a poll yields no value).
    title_unread: HashMap<Uuid, Unread>,
    /// Accounts whose last poll yielded a value (it then wins over the title).
    poll_active: HashSet<Uuid>,
    rules: HashMap<String, Rules>,
    last_badge: Option<DockBadge>,
    app_info: AppInfo,
    /// Accounts whose view currently exists.
    loaded: HashSet<Uuid>,
    /// Last time (tick clock) each account was active (shown) or left.
    last_active: HashMap<Uuid, u64>,
    /// Time of the last tick.
    now_ms: Option<u64>,
    /// Accounts whose favicon was already looked up in this run.
    /// Favicon lookups started this run, per account (retried on later page loads after a failure).
    icon_attempts: HashMap<Uuid, u8>,
    last_menus: Option<MenuModel>,
    /// Problems found while loading (logged on start).
    startup_log: Vec<String>,
}

/// Body of notifications when previews are off.
pub const HIDDEN_PREVIEW_BODY: &str = "New message";

#[derive(Default)]
struct Rules {
    direct: Option<Regex>,
    indirect: Option<Regex>,
}

impl Core {
    /// `id_gen` creates ids for new accounts (`Uuid::new_v4` in the app, deterministic in tests).
    /// `catalog` holds the built-in and user TOML services; the persisted custom services are
    /// added on top.
    pub fn new(
        catalog: Catalog,
        persisted: Persisted,
        id_gen: Box<dyn FnMut() -> Uuid + Send>,
    ) -> Core {
        let active = persisted.settings.active;
        let mut core = Core {
            catalog: catalog.clone(),
            builtin: catalog.clone(),
            base_catalog: catalog,
            user_defs: vec![],
            service_errors: vec![],
            persisted,
            id_gen,
            active,
            dialog_open: false,
            window: (0.0, 0.0),
            started: false,
            unread: HashMap::new(),
            title_unread: HashMap::new(),
            poll_active: HashSet::new(),
            rules: HashMap::new(),
            last_badge: None,
            app_info: AppInfo::default(),
            loaded: HashSet::new(),
            last_active: HashMap::new(),
            now_ms: None,
            icon_attempts: HashMap::new(),
            last_menus: None,
            startup_log: vec![],
        };
        core.startup_log = core.rebuild_catalog();
        core
    }

    /// Applies the user's service files found at startup (call before `Input::Started`).
    /// Their problems are logged on start and shown in the snapshot.
    pub fn set_user_services(&mut self, defs: Vec<ServiceDef>, errors: Vec<String>) {
        self.startup_log.extend(errors.iter().cloned());
        self.apply_user_services(defs, errors);
    }

    /// Replaces the user definitions (a failed file keeps the previous definitions of ids that
    /// no file defines any more, as the failed file may be the one that defined them) and
    /// rebuilds the catalog. Returns the problems of the custom services.
    fn apply_user_services(&mut self, defs: Vec<ServiceDef>, errors: Vec<String>) -> Vec<String> {
        let mut defs = defs;
        if !errors.is_empty() {
            let kept: Vec<ServiceDef> = self
                .user_defs
                .iter()
                .filter(|old| !defs.iter().any(|d| d.id == old.id))
                .cloned()
                .collect();
            defs.extend(kept);
        }
        let mut base = self.builtin.clone();
        for def in &defs {
            base.insert(def.clone());
        }
        self.user_defs = defs;
        self.base_catalog = base;
        self.service_errors = errors;
        self.rebuild_catalog()
    }

    /// Reload of the user's files: new catalog and unread rules; accounts whose view would now
    /// differ are recreated, the view policies are re-sent.
    fn services_loaded(&mut self, defs: Vec<ServiceDef>, errors: Vec<String>) -> Vec<Effect> {
        let old: Vec<(Uuid, Option<ViewSpec>)> = self
            .persisted
            .accounts
            .iter()
            .filter(|a| self.loaded.contains(&a.id))
            .map(|a| (a.id, self.view_spec(a).ok()))
            .collect();
        let mut fx: Vec<Effect> = errors.iter().cloned().map(Effect::Log).collect();
        fx.push(Effect::Log(format!(
            "reloaded services: {} user definitions, {} errors",
            defs.len(),
            errors.len()
        )));
        fx.extend(
            self.apply_user_services(defs, errors)
                .into_iter()
                .map(Effect::Log),
        );
        for (a, old_spec) in old {
            let Some(acc) = self.account(a) else { continue };
            let policy = self.view_policy(acc);
            let new_spec = self.view_spec(acc).ok();
            if new_spec.is_some() && new_spec != old_spec {
                fx.extend(self.unload(a));
                let visible = self.active == Some(a) && !self.dialog_open;
                fx.extend(self.create(a, visible));
            } else {
                fx.push(Effect::SetViewPolicy { account: a, policy });
            }
        }
        fx.push(self.emit());
        fx
    }

    /// Read-only app information shown in the snapshot ("About").
    pub fn set_app_info(&mut self, info: AppInfo) {
        self.app_info = info;
    }

    fn settings(&self) -> &AppSettings {
        &self.persisted.settings.app
    }

    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        let mut fx = match input {
            Input::Started { width, height } => self.started(width, height),
            Input::Ui(cmd) => self.ui(cmd),
            Input::Menu(action) => self.menu(action),
            Input::TitleChanged { account, title } => self.title_changed(account, &title),
            Input::PollResult { account, raw } => self.poll_result(account, &raw),
            Input::PageLoaded { account, .. } => self.page_loaded(account),
            Input::ExternalLink { account, url } => {
                if crate::security::opener_url_allowed(&url) {
                    vec![Effect::OpenExternal {
                        url,
                        browser: self.settings().browser.clone(),
                    }]
                } else {
                    vec![Effect::Log(format!(
                        "[{account}] not opening {url:.200}: scheme not allowed"
                    ))]
                }
            }
            Input::NotificationClicked { account, shim_id } => {
                let mut fx = self.select(account);
                if self.has_account(account) && self.is_loaded(account) {
                    fx.push(Effect::FocusWindow);
                    fx.push(Effect::Eval {
                        account,
                        script: format!("window.__msgr&&window.__msgr.click({shim_id})"),
                    });
                }
                fx
            }
            Input::WindowResized { width, height } => {
                self.window = (width, height);
                self.layout()
            }
            Input::Tick { now_ms } => self.tick(now_ms),
            Input::IconFound { account, raw } => self.icon_found(account, &raw),
            Input::IconFetched { account, data_url } => self.icon_fetched(account, data_url),
            Input::SimpleIconFetched {
                account,
                slug,
                result,
            } => self.simple_icon_fetched(account, &slug, result),
            Input::ServicesLoaded { defs, errors } => self.services_loaded(defs, errors),
        };
        if self.started {
            let menus = self.menu_model();
            if self.last_menus.as_ref() != Some(&menus) {
                self.last_menus = Some(menus.clone());
                fx.push(Effect::UpdateMenus(menus));
            }
        }
        fx
    }

    pub fn snapshot(&self) -> Snapshot {
        let accounts = self
            .persisted
            .accounts
            .iter()
            .map(|a| {
                let svc = self.catalog.get(&a.service);
                let service_name = svc
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| a.service.clone());
                let u = self.unread.get(&a.id).copied().unwrap_or_default();
                let basis = if a.label != service_name {
                    &a.label
                } else {
                    &service_name
                };
                let glyph = a.custom_icon.is_none()
                    && a.simple_icon.is_some()
                    && a.simple_icon_data.is_some();
                let icon = if glyph {
                    a.simple_icon_data.clone()
                } else {
                    a.custom_icon.clone().or_else(|| a.favicon.clone())
                };
                AccountView {
                    id: a.id,
                    service: a.service.clone(),
                    initials: initials(basis),
                    service_name,
                    color: svc.map(|s| s.color.clone()).unwrap_or_default(),
                    unread: u.direct,
                    indirect: u.indirect,
                    instance_label: svc.and_then(|s| s.instance_label.clone()),
                    icon,
                    icon_glyph: glyph,
                    hibernated: !a.enabled || (self.started && !self.loaded.contains(&a.id)),
                    settings: a.settings(),
                }
            })
            .collect();
        let services = self
            .catalog
            .services()
            .iter()
            .map(|s| ServiceInfo {
                id: s.id.clone(),
                name: s.name.clone(),
                color: s.color.clone(),
                instance_label: s.instance_label.clone(),
                instance_default: s.instance_default.clone(),
                custom: self.is_custom(&s.id),
                allow_media: s.allow_media,
                url: s.url.clone(),
                title_regex: s.unread.title_regex.clone(),
                user_agent: s.user_agent.clone(),
            })
            .collect();
        Snapshot {
            accounts,
            active: self.active,
            services,
            custom_services: self.persisted.custom_services.clone(),
            dialog_open: self.dialog_open,
            settings: self.settings().clone(),
            app: self.app_info.clone(),
            service_errors: self.service_errors.clone(),
        }
    }

    // ---- helpers ----

    /// Width of the sidebar (logical px); follows the icon size setting.
    fn sidebar_width(&self) -> f64 {
        self.settings().icon_size.sidebar_width()
    }

    fn content_bounds(&self) -> Bounds {
        let x = self.sidebar_width();
        Bounds {
            x,
            y: 0.0,
            width: (self.window.0 - x).max(0.0),
            height: self.window.1,
        }
    }

    fn sidebar_bounds(&self) -> Bounds {
        let width = if self.dialog_open {
            self.window.0
        } else {
            self.sidebar_width()
        };
        Bounds {
            x: 0.0,
            y: 0.0,
            width,
            height: self.window.1,
        }
    }

    /// Places the sidebar and the active view (unless the dialog covers the content) after the
    /// window size or the sidebar width changed.
    fn layout(&self) -> Vec<Effect> {
        let mut fx = vec![Effect::SetUiBounds(self.sidebar_bounds())];
        if let (Some(a), false) = (self.active, self.dialog_open)
            && self.is_loaded(a)
        {
            fx.push(Effect::SetBounds {
                account: a,
                bounds: self.content_bounds(),
            });
        }
        fx
    }

    fn persist(&mut self) -> Effect {
        self.persisted.settings.active = self.active;
        Effect::Persist(self.persisted.clone())
    }

    fn emit(&self) -> Effect {
        Effect::EmitSnapshot(self.snapshot())
    }

    fn has_account(&self, id: Uuid) -> bool {
        self.persisted.accounts.iter().any(|a| a.id == id)
    }

    fn account(&self, id: Uuid) -> Option<&Account> {
        self.persisted.accounts.iter().find(|a| a.id == id)
    }

    fn account_mut(&mut self, id: Uuid) -> Option<&mut Account> {
        self.persisted.accounts.iter_mut().find(|a| a.id == id)
    }

    fn is_loaded(&self, id: Uuid) -> bool {
        self.loaded.contains(&id)
    }

    fn is_custom(&self, service: &str) -> bool {
        self.persisted
            .custom_services
            .iter()
            .any(|c| c.id == service)
    }

    fn unknown(id: Uuid) -> Effect {
        Effect::Log(format!("unknown account {id}"))
    }

    fn not_loaded(id: Uuid, what: &str) -> Effect {
        Effect::Log(format!(
            "{what}: account {id} has no view (disabled or hibernated)"
        ))
    }

    /// Catalog = base + valid custom services; recompiles the unread rules. Returns problems.
    fn rebuild_catalog(&mut self) -> Vec<String> {
        let mut catalog = self.base_catalog.clone();
        let mut problems = vec![];
        for c in &self.persisted.custom_services {
            match ServiceDef::from_custom(c) {
                Ok(def) => catalog.insert(def),
                Err(e) => problems.push(e),
            }
        }
        self.rules = catalog
            .services()
            .iter()
            .map(|s| {
                let compile = |p: &Option<String>| p.as_deref().and_then(|p| Regex::new(p).ok());
                let r = Rules {
                    direct: compile(&s.unread.title_regex),
                    indirect: compile(&s.unread.title_indirect_regex),
                };
                (s.id.clone(), r)
            })
            .collect();
        self.catalog = catalog;
        problems
    }

    fn view_policy(&self, acc: &Account) -> ViewPolicy {
        let service_media = self
            .catalog
            .get(&acc.service)
            .is_some_and(|s| s.allow_media);
        ViewPolicy {
            link_target: acc.link_target.unwrap_or(self.settings().link_target),
            allow_media: acc.allow_media.unwrap_or(service_media),
            app_hosts: acc.app_hosts.clone(),
        }
    }

    fn badge(&self) -> DockBadge {
        let mut count = 0u32;
        let mut dot = false;
        for a in self
            .persisted
            .accounts
            .iter()
            .filter(|a| a.in_dock_badge && !a.muted && a.enabled)
        {
            let u = self.unread.get(&a.id).copied().unwrap_or_default();
            count = count.saturating_add(u.direct);
            dot |= u.indirect;
        }
        match self.settings().dock_badge {
            DockBadgeMode::Off => DockBadge::None,
            _ if count > 0 => DockBadge::Count(count),
            DockBadgeMode::All if dot => DockBadge::Dot,
            _ => DockBadge::None,
        }
    }

    /// Recomputes the dock badge; returns an effect only if it changed.
    fn dock_badge(&mut self) -> Option<Effect> {
        let badge = self.badge();
        if self.last_badge == Some(badge) {
            return None;
        }
        self.last_badge = Some(badge);
        Some(Effect::SetDockBadge(badge))
    }

    fn menu_model(&self) -> MenuModel {
        let tray_title = match self.badge() {
            DockBadge::Count(n) => n.to_string(),
            DockBadge::Dot => "•".to_string(),
            DockBadge::None => String::new(),
        };
        MenuModel {
            accounts: self
                .persisted
                .accounts
                .iter()
                .filter(|a| a.enabled)
                .map(|a| MenuAccount {
                    id: a.id,
                    label: a.label.clone(),
                    unread: self.unread.get(&a.id).map_or(0, |u| u.direct),
                })
                .collect(),
            dnd: self.settings().dnd,
            show_tray: self.settings().show_tray,
            tray_title,
        }
    }

    fn view_spec(&self, acc: &Account) -> Result<ViewSpec, String> {
        let svc = self
            .catalog
            .get(&acc.service)
            .ok_or_else(|| format!("account {}: unknown service {}", acc.id, acc.service))?;
        let instance = acc.instance.as_deref();
        let url = svc
            .resolve_url(instance)
            .map_err(|e| format!("account {}: {e}", acc.id))?;
        let user_agent = match &acc.user_agent {
            Some(preset) => crate::catalog::resolve_user_agent_preset(Some(preset)),
            None => svc.resolve_user_agent(),
        };
        let mut init_script = String::new();
        if !svc.clear_local_storage.is_empty() {
            init_script.push_str(&storage_cleaner(&svc.clear_local_storage));
            init_script.push('\n');
        }
        init_script.push_str(crate::NOTIFY_SHIM);
        if let Some(css) = acc.custom_css.as_deref().filter(|c| !c.trim().is_empty()) {
            init_script.push('\n');
            init_script.push_str(&css_injector(css));
        }
        Ok(ViewSpec {
            account: acc.id,
            url,
            store_id: acc.store_id(),
            user_agent,
            init_script,
            internal_hosts: svc.resolve_hosts(instance),
        })
    }

    /// `SetViewPolicy`, `CreateView`, `SetZoom` for an account (marks it loaded), or a log line.
    fn create(&mut self, a: Uuid, visible: bool) -> Vec<Effect> {
        let Some(acc) = self.account(a) else {
            return vec![Self::unknown(a)];
        };
        match self.view_spec(acc) {
            Ok(spec) => {
                let (policy, zoom) = (self.view_policy(acc), acc.zoom);
                self.loaded.insert(a);
                vec![
                    Effect::SetViewPolicy { account: a, policy },
                    Effect::CreateView {
                        spec,
                        bounds: self.content_bounds(),
                        visible,
                    },
                    Effect::SetZoom { account: a, zoom },
                ]
            }
            Err(e) => vec![Effect::Log(e)],
        }
    }

    /// `Destroy` if the account has a view.
    fn unload(&mut self, a: Uuid) -> Vec<Effect> {
        if self.loaded.remove(&a) {
            vec![Effect::Destroy(a)]
        } else {
            vec![]
        }
    }

    /// Shows the active account's view (creating it if it is not loaded), unless the dialog
    /// covers the content.
    fn show_active(&mut self) -> Vec<Effect> {
        match (self.active, self.dialog_open) {
            (Some(a), false) if self.is_loaded(a) => {
                vec![
                    Effect::SetBounds {
                        account: a,
                        bounds: self.content_bounds(),
                    },
                    Effect::Show(a),
                ]
            }
            (Some(a), false) if self.started => self.create(a, true),
            _ => vec![],
        }
    }

    /// Nearest enabled account to position `idx` (at or after it first, then before), skipping `except`.
    fn neighbor(&self, idx: usize, except: Option<Uuid>) -> Option<Uuid> {
        let accounts = &self.persisted.accounts;
        let ok = |a: &&Account| a.enabled && Some(a.id) != except;
        accounts
            .iter()
            .skip(idx)
            .find(ok)
            .or_else(|| accounts.iter().take(idx).rev().find(ok))
            .map(|a| a.id)
    }

    fn mark_left(&mut self, a: Uuid) {
        if let Some(now) = self.now_ms {
            self.last_active.insert(a, now);
        }
    }

    // ---- inputs ----

    fn started(&mut self, width: f64, height: f64) -> Vec<Effect> {
        self.window = (width, height);
        self.started = true;
        self.last_badge = None;
        if !self
            .active
            .is_some_and(|a| self.account(a).is_some_and(|x| x.enabled))
        {
            self.active = self.neighbor(0, None);
        }
        self.persisted.settings.active = self.active;
        let mut fx = vec![
            Effect::ApplySystemSettings(self.settings().clone()),
            Effect::SetUiBounds(self.sidebar_bounds()),
        ];
        fx.extend(
            std::mem::take(&mut self.startup_log)
                .into_iter()
                .map(Effect::Log),
        );
        let enabled: Vec<Uuid> = self
            .persisted
            .accounts
            .iter()
            .filter(|a| a.enabled)
            .map(|a| a.id)
            .collect();
        for a in enabled {
            let visible = Some(a) == self.active;
            fx.extend(self.create(a, visible));
        }
        let missing: Vec<(Uuid, String)> = self
            .persisted
            .accounts
            .iter()
            .filter(|a| a.simple_icon_data.is_none())
            .filter_map(|a| a.simple_icon.clone().map(|s| (a.id, s)))
            .collect();
        fx.extend(
            missing
                .into_iter()
                .map(|(account, slug)| Effect::FetchSimpleIcon { account, slug }),
        );
        fx.extend(self.dock_badge());
        fx.push(self.emit());
        fx
    }

    /// Selecting a disabled account is refused (logged): it has no view to show, and the
    /// UI enables it through the account settings.
    fn select(&mut self, a: Uuid) -> Vec<Effect> {
        let Some(acc) = self.account(a) else {
            return vec![Self::unknown(a)];
        };
        if !acc.enabled {
            return vec![
                Effect::Log(format!("select: account {a} is disabled")),
                self.emit(),
            ];
        }
        let mut fx = vec![];
        if self.dialog_open {
            self.dialog_open = false;
            fx.push(Effect::SetUiBounds(self.sidebar_bounds()));
        }
        if let Some(old) = self.active.filter(|&old| old != a) {
            if self.is_loaded(old) {
                fx.push(Effect::Hide(old));
            }
            self.mark_left(old);
        }
        self.active = Some(a);
        self.mark_left(a);
        fx.extend(self.show_active());
        fx.push(self.persist());
        fx.push(self.emit());
        fx
    }

    fn ui(&mut self, cmd: UiCommand) -> Vec<Effect> {
        match cmd {
            UiCommand::Select { account } => self.select(account),
            UiCommand::AddAccount {
                service,
                label,
                instance,
            } => self.add_account(&service, &label, instance),
            UiCommand::RemoveAccount { account } => self.remove_account(account),
            UiCommand::SetMuted { account, muted } => {
                let Some(acc) = self.account_mut(account) else {
                    return vec![Self::unknown(account)];
                };
                acc.muted = muted;
                let mut fx = vec![self.persist()];
                fx.extend(self.dock_badge());
                fx.push(self.emit());
                fx
            }
            UiCommand::Move { account, delta } => {
                let accounts = &mut self.persisted.accounts;
                let Some(idx) = accounts.iter().position(|a| a.id == account) else {
                    return vec![Self::unknown(account)];
                };
                let max = accounts.len() as i64 - 1;
                let to = (idx as i64 + i64::from(delta)).clamp(0, max) as usize;
                let acc = accounts.remove(idx);
                accounts.insert(to, acc);
                vec![self.persist(), self.emit()]
            }
            UiCommand::Reload { account } => {
                self.on_view(account, "reload", |a| vec![Effect::Reload(a)])
            }
            UiCommand::SetDialog { open } => {
                if open == self.dialog_open {
                    return vec![self.emit()];
                }
                self.dialog_open = open;
                let mut fx = vec![Effect::SetUiBounds(self.sidebar_bounds())];
                if open {
                    if let Some(a) = self.active.filter(|&a| self.is_loaded(a)) {
                        fx.push(Effect::Hide(a));
                    }
                } else {
                    fx.extend(self.show_active());
                }
                fx.push(self.emit());
                fx
            }
            UiCommand::SetDnd { enabled } => {
                self.persisted.settings.app.dnd = enabled;
                vec![self.persist(), self.emit()]
            }
            UiCommand::UpdateSettings { settings } => self.update_settings(settings),
            UiCommand::UpdateAccount { account, settings } => {
                self.update_account(account, settings)
            }
            UiCommand::GoBack { account } => self.on_view(account, "back", |a| {
                vec![Effect::Eval {
                    account: a,
                    script: "history.back()".into(),
                }]
            }),
            UiCommand::GoForward { account } => self.on_view(account, "forward", |a| {
                vec![Effect::Eval {
                    account: a,
                    script: "history.forward()".into(),
                }]
            }),
            UiCommand::OpenInBrowser { account } => self.open_in_browser(account),
            UiCommand::ClearData { account } => {
                let Some(acc) = self.account(account) else {
                    return vec![Self::unknown(account)];
                };
                let store_id = acc.store_id();
                let effect = if self.is_loaded(account) {
                    Effect::ClearData(account)
                } else {
                    Effect::DeleteProfile { store_id }
                };
                vec![
                    Effect::Log(format!("clearing browsing data of account {account}")),
                    effect,
                ]
            }
            UiCommand::Hibernate { account } => self.hibernate(account),
            UiCommand::RefreshIcon { account } => self.on_view(account, "refresh icon", |a| {
                vec![Effect::FindIcon {
                    account: a,
                    script: ICON_SCRIPT.into(),
                }]
            }),
            UiCommand::AddCustomService { service } => self.add_custom_service(service),
            UiCommand::UpdateCustomService { service } => self.update_custom_service(service),
            UiCommand::RemoveCustomService { id } => self.remove_custom_service(&id),
            UiCommand::OpenServicesFolder => vec![Effect::OpenServicesFolder],
            UiCommand::ReloadServices => vec![Effect::LoadServices],
            UiCommand::OpenProjectPage => vec![Effect::OpenExternal {
                url: protocol::PROJECT_URL.into(),
                browser: self.settings().browser.clone(),
            }],
        }
    }

    /// Runs `f` for an account with a view; logs unknown or unloaded accounts.
    fn on_view(&mut self, a: Uuid, what: &str, f: impl FnOnce(Uuid) -> Vec<Effect>) -> Vec<Effect> {
        if !self.has_account(a) {
            vec![Self::unknown(a)]
        } else if !self.is_loaded(a) {
            vec![Self::not_loaded(a, what)]
        } else {
            f(a)
        }
    }

    fn open_in_browser(&mut self, a: Uuid) -> Vec<Effect> {
        let Some(acc) = self.account(a) else {
            return vec![Self::unknown(a)];
        };
        let browser = self.settings().browser.clone();
        if self.is_loaded(a) {
            return vec![Effect::OpenInBrowser {
                account: a,
                browser,
            }];
        }
        // No view: open the service's start page instead.
        match self.view_spec(acc) {
            // Service URLs are always http(s) (checked by `resolve_url`).
            Ok(spec) => vec![Effect::OpenExternal {
                url: spec.url.to_string(),
                browser,
            }],
            Err(e) => vec![Effect::Log(e)],
        }
    }

    fn menu(&mut self, action: MenuAction) -> Vec<Effect> {
        if action == MenuAction::ToggleDnd {
            let enabled = !self.settings().dnd;
            return self.ui(UiCommand::SetDnd { enabled });
        }
        let Some(a) = self.active else {
            return vec![Effect::Log(format!("menu {action:?}: no active account"))];
        };
        match action {
            MenuAction::Reload => self.ui(UiCommand::Reload { account: a }),
            MenuAction::Back => self.ui(UiCommand::GoBack { account: a }),
            MenuAction::Forward => self.ui(UiCommand::GoForward { account: a }),
            MenuAction::OpenInBrowser => self.open_in_browser(a),
            MenuAction::ZoomIn | MenuAction::ZoomOut | MenuAction::ZoomReset => {
                let current = self.account(a).map_or(1.0, |x| x.zoom);
                let zoom = match action {
                    MenuAction::ZoomIn => ZOOM_STEPS.iter().copied().find(|&z| z > current + 1e-3),
                    MenuAction::ZoomOut => ZOOM_STEPS
                        .iter()
                        .rev()
                        .copied()
                        .find(|&z| z < current - 1e-3),
                    _ => Some(1.0),
                };
                match zoom {
                    Some(zoom) => self.set_zoom(a, zoom),
                    None => vec![],
                }
            }
            MenuAction::ToggleDnd => unreachable!("handled above"),
        }
    }

    fn set_zoom(&mut self, a: Uuid, zoom: f64) -> Vec<Effect> {
        let zoom = sanitize_zoom(zoom);
        let Some(acc) = self.account_mut(a) else {
            return vec![Self::unknown(a)];
        };
        if acc.zoom == zoom {
            return vec![];
        }
        acc.zoom = zoom;
        let mut fx = vec![];
        if self.is_loaded(a) {
            fx.push(Effect::SetZoom { account: a, zoom });
        }
        fx.push(self.persist());
        fx.push(self.emit());
        fx
    }

    fn update_settings(&mut self, mut settings: AppSettings) -> Vec<Effect> {
        settings.browser = trimmed(settings.browser);
        let (old_target, old_icon_size) = (self.settings().link_target, self.settings().icon_size);
        self.persisted.settings.app = settings;
        let mut fx = vec![
            self.persist(),
            Effect::ApplySystemSettings(self.settings().clone()),
        ];
        if self.started && old_icon_size != self.settings().icon_size {
            fx.extend(self.layout());
        }
        if old_target != self.settings().link_target {
            for acc in self
                .persisted
                .accounts
                .iter()
                .filter(|a| a.link_target.is_none() && self.loaded.contains(&a.id))
            {
                fx.push(Effect::SetViewPolicy {
                    account: acc.id,
                    policy: self.view_policy(acc),
                });
            }
        }
        fx.extend(self.dock_badge());
        fx.push(self.emit());
        fx
    }

    fn update_account(&mut self, a: Uuid, settings: AccountSettings) -> Vec<Effect> {
        let Some(acc) = self.account(a) else {
            return vec![Self::unknown(a)];
        };
        let Some(svc) = self.catalog.get(&acc.service) else {
            return vec![
                Effect::Log(format!(
                    "update account {a}: unknown service {}",
                    acc.service
                )),
                self.emit(),
            ];
        };
        let instance = settings
            .instance
            .map(|t| crate::catalog::normalize_instance(&t))
            .filter(|t| !t.is_empty());
        if let Err(e) = svc.resolve_url(instance.as_deref()) {
            return vec![Effect::Log(format!("update account {a}: {e}")), self.emit()];
        }
        let label = settings.label.trim();
        let label = if label.is_empty() {
            svc.name.clone()
        } else {
            label.to_string()
        };
        let user_agent = trimmed(settings.user_agent);
        let custom_css = settings.custom_css.filter(|c| !c.trim().is_empty());
        let zoom = sanitize_zoom(settings.zoom);
        let (app_hosts, rejected_hosts) = crate::catalog::normalize_app_hosts(&settings.app_hosts);
        let recreate = acc.instance != instance
            || acc.user_agent != user_agent
            || acc.custom_css != custom_css;
        let (was_enabled, old_policy, old_zoom) = (acc.enabled, self.view_policy(acc), acc.zoom);

        let (slug, slug_fx) = match normalize_slug(settings.simple_icon.as_deref().unwrap_or("")) {
            Ok(s) => (s, None),
            Err(e) => (None, Some(Effect::Log(format!("update account {a}: {e}")))),
        };
        let slug_changed = acc.simple_icon != slug;

        let acc = self.account_mut(a).expect("checked above");
        if slug_changed {
            acc.simple_icon = slug.clone();
            acc.simple_icon_data = None;
        }
        acc.label = label;
        acc.instance = instance;
        acc.muted = settings.muted;
        acc.show_badge = settings.show_badge;
        acc.in_dock_badge = settings.in_dock_badge;
        acc.user_agent = user_agent;
        acc.enabled = settings.enabled;
        acc.link_target = settings.link_target;
        acc.custom_icon = trimmed(settings.custom_icon);
        acc.zoom = zoom;
        acc.hibernate_after_min = settings.hibernate_after_min;
        acc.custom_css = custom_css;
        acc.allow_media = settings.allow_media;
        acc.app_hosts = app_hosts;
        let new_policy = self.view_policy(self.account(a).expect("checked above"));

        let mut fx = vec![];
        if !self.started {
            // Views are created on start with the new settings.
        } else if was_enabled && !settings.enabled {
            fx.extend(self.unload(a));
            self.unread.remove(&a);
            self.title_unread.remove(&a);
            self.poll_active.remove(&a);
            if self.active == Some(a) {
                let idx = self
                    .persisted
                    .accounts
                    .iter()
                    .position(|x| x.id == a)
                    .unwrap_or(0);
                self.active = self.neighbor(idx, Some(a));
                fx.extend(self.show_active());
            }
        } else if !was_enabled && settings.enabled {
            if self.active.is_none() {
                self.active = Some(a);
            }
            let visible = self.active == Some(a) && !self.dialog_open;
            fx.extend(self.create(a, visible));
        } else if self.is_loaded(a) {
            if recreate {
                fx.extend(self.unload(a));
                let visible = self.active == Some(a) && !self.dialog_open;
                fx.extend(self.create(a, visible));
            } else {
                if old_policy != new_policy {
                    fx.push(Effect::SetViewPolicy {
                        account: a,
                        policy: new_policy,
                    });
                }
                if old_zoom != zoom {
                    fx.push(Effect::SetZoom { account: a, zoom });
                }
            }
        }
        fx.extend(slug_fx);
        for r in rejected_hosts {
            fx.push(Effect::Log(format!(
                "update account {a}: ignoring invalid app host {r:?}"
            )));
        }
        if slug_changed
            && self.started
            && let Some(slug) = slug
        {
            fx.push(Effect::FetchSimpleIcon { account: a, slug });
        }
        fx.push(self.persist());
        fx.extend(self.dock_badge());
        fx.push(self.emit());
        fx
    }

    fn add_account(&mut self, service: &str, label: &str, instance: Option<String>) -> Vec<Effect> {
        let instance = instance
            .map(|t| crate::catalog::normalize_instance(&t))
            .filter(|t| !t.is_empty());
        let Some(svc) = self.catalog.get(service) else {
            return vec![
                Effect::Log(format!("add account: unknown service {service}")),
                self.emit(),
            ];
        };
        if let Err(e) = svc.resolve_url(instance.as_deref()) {
            return vec![Effect::Log(format!("add account: {e}")), self.emit()];
        }
        let label = label.trim();
        let label = if label.is_empty() {
            svc.name.clone()
        } else {
            label.to_string()
        };
        let acc = Account::new((self.id_gen)(), service, label, instance);
        let id = acc.id;
        self.persisted.accounts.push(acc);
        let mut fx = vec![];
        if self.started && self.dialog_open {
            // Added from the settings dialog: keep the dialog open (the user may add more) and load
            // the view hidden. It only becomes active if nothing else is; closing the dialog shows it.
            fx.extend(self.create(id, false));
            if self.active.is_none_or(|a| !self.has_account(a)) && self.is_loaded(id) {
                self.active = Some(id);
                self.mark_left(id);
            }
        } else if self.started {
            let old = self.active.filter(|&old| self.is_loaded(old));
            let created = self.create(id, true);
            if self.is_loaded(id) {
                if let Some(old) = old {
                    fx.push(Effect::Hide(old));
                    self.mark_left(old);
                }
                self.active = Some(id);
                self.mark_left(id);
            }
            fx.extend(created);
        }
        fx.push(self.persist());
        fx.push(self.emit());
        fx
    }

    fn remove_account(&mut self, a: Uuid) -> Vec<Effect> {
        let Some(idx) = self.persisted.accounts.iter().position(|x| x.id == a) else {
            return vec![Self::unknown(a)];
        };
        let acc = self.persisted.accounts.remove(idx);
        self.unread.remove(&a);
        self.title_unread.remove(&a);
        self.poll_active.remove(&a);
        self.last_active.remove(&a);
        self.icon_attempts.remove(&a);
        let mut fx = self.unload(a);
        fx.push(Effect::DeleteProfile {
            store_id: acc.store_id(),
        });
        if self.active == Some(a) {
            self.active = self.neighbor(idx, None);
            fx.extend(self.show_active());
        }
        fx.push(self.persist());
        fx.extend(self.dock_badge());
        fx.push(self.emit());
        fx
    }

    fn hibernate(&mut self, a: Uuid) -> Vec<Effect> {
        if !self.has_account(a) {
            return vec![Self::unknown(a)];
        }
        if self.active == Some(a) {
            return vec![Effect::Log(format!(
                "hibernate: not unloading the active account {a}"
            ))];
        }
        let mut fx = self.unload(a);
        if !fx.is_empty() {
            fx.push(Effect::Log(format!("hibernated account {a}")));
        }
        fx.push(self.emit());
        fx
    }

    /// Stores new unread state; returns the follow-up effects if it changed.
    fn update_unread(&mut self, a: Uuid, new: Unread) -> Vec<Effect> {
        let old = self.unread.get(&a).copied().unwrap_or_default();
        if old == new {
            return vec![];
        }
        self.unread.insert(a, new);
        let mut fx: Vec<Effect> = self.dock_badge().into_iter().collect();
        fx.push(self.emit());
        fx
    }

    fn title_changed(&mut self, a: Uuid, title: &str) -> Vec<Effect> {
        let Some(acc) = self.account(a) else {
            return vec![Self::unknown(a)];
        };
        let service = acc.service.clone();
        let Some(svc) = self.catalog.get(&service) else {
            return vec![];
        };
        let has_poll = svc.unread.poll.is_some();
        let rules = self.rules.get(&service);
        let direct = rules
            .and_then(|r| r.direct.as_ref())
            .and_then(|re| re.captures(title))
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(0);
        let indirect = rules
            .and_then(|r| r.indirect.as_ref())
            .is_some_and(|re| re.is_match(title));
        let new = Unread { direct, indirect };
        self.title_unread.insert(a, new);
        if has_poll && self.poll_active.contains(&a) {
            return vec![];
        }
        let mut fx = Vec::new();
        if self.unread.get(&a).copied().unwrap_or_default() != new {
            // Lets the user check the title patterns against what the service really shows.
            fx.push(Effect::Log(format!(
                "[{a}] {service} title {title:?} -> unread {direct}, indirect {indirect}"
            )));
        }
        fx.extend(self.update_unread(a, new));
        fx
    }

    fn poll_result(&mut self, a: Uuid, raw: &str) -> Vec<Effect> {
        let Some(acc) = self.account(a) else {
            return vec![Self::unknown(a)];
        };
        let (muted, label, service) = (acc.muted, acc.label.clone(), acc.service.clone());
        let Some(svc) = self.catalog.get(&service) else {
            return vec![Effect::Log(format!(
                "poll result: unknown service {service}"
            ))];
        };
        let has_poll = svc.unread.poll.is_some();
        let name = svc.name.clone();

        let value = serde_json::from_str::<serde_json::Value>(raw).and_then(|v| match v {
            serde_json::Value::String(s) => serde_json::from_str(&s),
            other => Ok(other),
        });
        let value = match value {
            Ok(v) => v,
            Err(e) => return vec![Effect::Log(format!("poll result for {a}: bad JSON: {e}"))],
        };
        #[derive(serde::Deserialize)]
        struct Item {
            id: u64,
            title: String,
            #[serde(default)]
            body: Option<String>,
        }
        let items: Vec<Item> = match value.get("n") {
            None | Some(serde_json::Value::Null) => vec![],
            Some(n) => match serde_json::from_value(n.clone()) {
                Ok(items) => items,
                Err(e) => {
                    return vec![Effect::Log(format!(
                        "poll result for {a}: bad notifications: {e}"
                    ))];
                }
            },
        };

        let mut fx = vec![];
        let settings = self.settings();
        if !muted && !settings.dnd {
            for it in items {
                let title = if label != name {
                    format!("{name} ({label}): {}", it.title)
                } else {
                    format!("{name}: {}", it.title)
                };
                fx.push(Effect::Notify(NotificationReq {
                    account: a,
                    shim_id: it.id,
                    title,
                    body: if settings.notification_preview {
                        it.body.unwrap_or_default()
                    } else {
                        HIDDEN_PREVIEW_BODY.to_string()
                    },
                    sound: settings.notification_sound,
                }));
            }
        }

        if has_poll {
            let new = match value.get("u") {
                Some(serde_json::Value::Number(n)) => {
                    let n = n
                        .as_u64()
                        .or_else(|| n.as_f64().map(|f| f.max(0.0) as u64))
                        .unwrap_or(0);
                    Some(Unread {
                        direct: n.min(u64::from(u32::MAX)) as u32,
                        indirect: false,
                    })
                }
                Some(obj @ serde_json::Value::Object(_)) => {
                    let direct = obj
                        .get("direct")
                        .and_then(|d| d.as_f64())
                        .unwrap_or(0.0)
                        .max(0.0);
                    let indirect = obj
                        .get("indirect")
                        .and_then(|i| i.as_bool())
                        .unwrap_or(false);
                    Some(Unread {
                        direct: direct as u32,
                        indirect,
                    })
                }
                _ => None,
            };
            match new {
                Some(new) => {
                    self.poll_active.insert(a);
                    if self.unread.get(&a).copied().unwrap_or_default() != new {
                        fx.push(Effect::Log(format!(
                            "[{a}] {service} poll -> unread {}, indirect {}",
                            new.direct, new.indirect
                        )));
                    }
                    fx.extend(self.update_unread(a, new));
                }
                None => {
                    // No value (element not found): fall back to the title-derived unread.
                    if self.poll_active.remove(&a) {
                        fx.push(Effect::Log(format!(
                            "[{a}] {service} poll -> no value, using title"
                        )));
                    }
                    if let Some(t) = self.title_unread.get(&a).copied() {
                        fx.extend(self.update_unread(a, t));
                    }
                }
            }
        }
        fx
    }

    fn tick(&mut self, now: u64) -> Vec<Effect> {
        if !self.started {
            return vec![];
        }
        let mut fx = vec![];
        let woke = self
            .now_ms
            .is_some_and(|last| now.saturating_sub(last) > WAKE_GAP_MS);
        if woke {
            let gap = (now - self.now_ms.unwrap_or(now)) / 1000;
            fx.push(Effect::Log(format!(
                "wake detected ({gap} s since the last tick)"
            )));
        }
        self.now_ms = Some(now);
        if let Some(a) = self.active {
            self.last_active.insert(a, now);
        }

        // Hibernate background accounts that were idle long enough.
        let global = self.settings().hibernate_after_min;
        let mut idle = vec![];
        for acc in &self.persisted.accounts {
            if Some(acc.id) == self.active || !self.loaded.contains(&acc.id) {
                continue;
            }
            // tracked even without a timeout, so a later setting counts from the last activity
            let since = *self.last_active.entry(acc.id).or_insert(now);
            // account override wins; 0 or None (without global) means never
            let Some(minutes) = acc.hibernate_after_min.or(global).filter(|&m| m > 0) else {
                continue;
            };
            if now.saturating_sub(since) >= u64::from(minutes) * 60_000 {
                idle.push((acc.id, minutes));
            }
        }
        for (a, minutes) in &idle {
            fx.extend(self.unload(*a));
            fx.push(Effect::Log(format!(
                "hibernated account {a} after {minutes} min in the background"
            )));
        }

        if woke && self.settings().reload_after_wake {
            for acc in self
                .persisted
                .accounts
                .iter()
                .filter(|a| self.loaded.contains(&a.id))
            {
                fx.push(Effect::Reload(acc.id));
            }
        }

        for acc in self
            .persisted
            .accounts
            .iter()
            .filter(|a| self.loaded.contains(&a.id))
        {
            let Some(svc) = self.catalog.get(&acc.service) else {
                continue;
            };
            let expr = svc.unread.poll.as_deref().unwrap_or("null");
            let script = format!(
                "(()=>{{const m=window.__msgr;let u=null;try{{u=({expr})}}catch(e){{}}return JSON.stringify({{n:m?m.drain():[],u:u}});}})()"
            );
            fx.push(Effect::Poll {
                account: acc.id,
                script,
            });
        }
        if !idle.is_empty() {
            fx.push(self.emit());
        }
        fx
    }

    // ---- favicons ----

    fn page_loaded(&mut self, a: Uuid) -> Vec<Effect> {
        let Some(acc) = self.account(a) else {
            return vec![];
        };
        if acc.favicon.is_some() || !self.is_loaded(a) {
            return vec![];
        }
        // Single-page apps often set their icon only after the first load: allow a few attempts,
        // each on a later page load (a successful fetch sets `favicon` and stops this).
        let attempts = self.icon_attempts.entry(a).or_insert(0);
        if *attempts >= MAX_ICON_ATTEMPTS {
            return vec![];
        }
        *attempts += 1;
        vec![Effect::FindIcon {
            account: a,
            script: ICON_SCRIPT.into(),
        }]
    }

    fn icon_found(&mut self, a: Uuid, raw: &str) -> Vec<Effect> {
        if !self.has_account(a) {
            return vec![Self::unknown(a)];
        }
        let mut value = serde_json::from_str::<serde_json::Value>(raw).ok();
        // Tolerate a doubly encoded string.
        if let Some(serde_json::Value::String(s)) = &value
            && s.starts_with('"')
        {
            value = serde_json::from_str(s).ok();
        }
        let url = match value {
            Some(serde_json::Value::String(s)) if !s.is_empty() => s,
            _ => {
                return vec![Effect::Log(format!(
                    "icon of account {a}: no icon URL ({raw:.100})"
                ))];
            }
        };
        if url.starts_with("data:image/") {
            return self.icon_fetched(a, Some(url));
        }
        match url::Url::parse(&url) {
            Ok(u) if matches!(u.scheme(), "http" | "https") => {
                vec![Effect::FetchIcon { account: a, url }]
            }
            _ => vec![Effect::Log(format!(
                "icon of account {a}: unsupported URL {url:.100}"
            ))],
        }
    }

    fn icon_fetched(&mut self, a: Uuid, data_url: Option<String>) -> Vec<Effect> {
        let Some(acc) = self.account_mut(a) else {
            return vec![Self::unknown(a)];
        };
        match data_url {
            Some(d) if d.starts_with("data:image/") => {
                if acc.favicon.as_deref() == Some(d.as_str()) {
                    return vec![];
                }
                acc.favicon = Some(d);
                vec![self.persist(), self.emit()]
            }
            _ => vec![Effect::Log(format!("icon of account {a}: download failed"))],
        }
    }

    fn simple_icon_fetched(
        &mut self,
        a: Uuid,
        slug: &str,
        result: SimpleIconResult,
    ) -> Vec<Effect> {
        let Some(acc) = self.account_mut(a) else {
            return vec![Self::unknown(a)];
        };
        if acc.simple_icon.as_deref() != Some(slug) {
            return vec![]; // stale: the slug changed meanwhile
        }
        match result {
            SimpleIconResult::Found(d) if d.starts_with("data:image/") => {
                acc.simple_icon_data = Some(d);
                vec![self.persist(), self.emit()]
            }
            SimpleIconResult::Found(_) => {
                vec![Effect::Log(format!("Simple Icons '{slug}': not an image"))]
            }
            SimpleIconResult::NotFound => {
                vec![Effect::Log(format!("Simple Icons has no icon '{slug}'"))]
            }
            SimpleIconResult::Failed(e) => {
                vec![Effect::Log(format!("Simple Icons '{slug}' failed: {e}"))]
            }
        }
    }

    // ---- custom services ----

    fn custom_error(&self, e: String) -> Vec<Effect> {
        vec![Effect::Log(e), self.emit()]
    }

    fn add_custom_service(&mut self, service: CustomService) -> Vec<Effect> {
        let mut c = match crate::catalog::normalize_custom(&service) {
            Ok(c) => c,
            Err(e) => return self.custom_error(format!("add custom service: {e}")),
        };
        c.id = crate::catalog::custom_service_id(&c.name, |id| {
            self.base_catalog.get(id).is_some()
                || self.persisted.custom_services.iter().any(|x| x.id == id)
        });
        let id = c.id.clone();
        self.persisted.custom_services.push(c);
        let mut fx: Vec<Effect> = self
            .rebuild_catalog()
            .into_iter()
            .map(Effect::Log)
            .collect();
        fx.push(Effect::Log(format!("added custom service {id}")));
        fx.push(self.persist());
        fx.push(self.emit());
        fx
    }

    fn update_custom_service(&mut self, service: CustomService) -> Vec<Effect> {
        let Some(idx) = self
            .persisted
            .custom_services
            .iter()
            .position(|c| c.id == service.id)
        else {
            return self.custom_error(format!(
                "update custom service: unknown id {:?}",
                service.id
            ));
        };
        let c = match crate::catalog::normalize_custom(&service) {
            Ok(c) => c,
            Err(e) => return self.custom_error(format!("update custom service: {e}")),
        };
        let id = c.id.clone();
        self.persisted.custom_services[idx] = c;
        let mut fx: Vec<Effect> = self
            .rebuild_catalog()
            .into_iter()
            .map(Effect::Log)
            .collect();
        let users: Vec<Uuid> = self
            .persisted
            .accounts
            .iter()
            .filter(|a| a.service == id && self.loaded.contains(&a.id))
            .map(|a| a.id)
            .collect();
        for a in users {
            fx.extend(self.unload(a));
            let visible = self.active == Some(a) && !self.dialog_open;
            fx.extend(self.create(a, visible));
        }
        fx.push(self.persist());
        fx.push(self.emit());
        fx
    }

    fn remove_custom_service(&mut self, id: &str) -> Vec<Effect> {
        let Some(idx) = self
            .persisted
            .custom_services
            .iter()
            .position(|c| c.id == id)
        else {
            return self.custom_error(format!("remove custom service: unknown id {id:?}"));
        };
        let users = self
            .persisted
            .accounts
            .iter()
            .filter(|a| a.service == id)
            .count();
        if users > 0 {
            return self.custom_error(format!(
                "remove custom service {id}: still used by {users} account(s)"
            ));
        }
        self.persisted.custom_services.remove(idx);
        let mut fx: Vec<Effect> = self
            .rebuild_catalog()
            .into_iter()
            .map(Effect::Log)
            .collect();
        fx.push(self.persist());
        fx.push(self.emit());
        fx
    }
}

/// Clamps a zoom factor to a sane range; non-finite values become 1.0.
fn sanitize_zoom(zoom: f64) -> f64 {
    if zoom.is_finite() {
        zoom.clamp(ZOOM_MIN, ZOOM_MAX)
    } else {
        1.0
    }
}

/// Script that adds `css` as a `<style>` element (at document start, again after
/// DOMContentLoaded if the page dropped it).
/// Removes the given `localStorage` keys at document start (before page scripts run).
fn storage_cleaner(keys: &[String]) -> String {
    let keys = serde_json::to_string(keys).expect("strings always serialize");
    format!("(()=>{{try{{for(const k of {keys})localStorage.removeItem(k);}}catch(e){{}}}})();")
}

fn css_injector(css: &str) -> String {
    let css = serde_json::to_string(css).expect("strings always serialize");
    format!(
        "(()=>{{const css={css};const add=()=>{{if(document.getElementById('__msgr_css'))return;const s=document.createElement('style');s.id='__msgr_css';s.textContent=css;(document.head||document.documentElement).appendChild(s);}};if(document.documentElement)add();document.addEventListener('DOMContentLoaded',add);}})();"
    )
}

/// Trims an optional text setting; empty means unset.
fn trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn initials(s: &str) -> String {
    fn upper(c: char) -> String {
        c.to_uppercase().collect()
    }
    let words: Vec<&str> = s.split_whitespace().collect();
    match words.as_slice() {
        [] => String::new(),
        [one] => {
            let mut cs = one.chars();
            let first = cs.next().map(upper).unwrap_or_default();
            first + &cs.next().map(String::from).unwrap_or_default()
        }
        [a, b, ..] => {
            let f = |w: &str| w.chars().next().map(upper).unwrap_or_default();
            f(a) + &f(b)
        }
    }
}

#[cfg(test)]
mod tests;
