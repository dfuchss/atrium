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
        if self
            .catalog
            .get(&service)
            .is_none_or(|s| s.unread.poll.is_some())
        {
            return vec![];
        }
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
            if let Some(new) = new {
                fx.extend(self.update_unread(a, new));
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
mod tests {
    use super::*;
    use protocol::IconSize;

    /// Sidebar width at the default icon size.
    const SIDEBAR: f64 = 56.0;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn acc(n: u128, service: &str, label: &str) -> Account {
        Account::new(id(n), service, label, None)
    }

    fn core_with(accounts: Vec<Account>) -> Core {
        let mut counter = 100u128;
        let gen_id = Box::new(move || {
            counter += 1;
            Uuid::from_u128(counter)
        });
        let persisted = Persisted {
            accounts,
            ..Default::default()
        };
        Core::new(Catalog::builtin(), persisted, gen_id)
    }

    fn started(accounts: Vec<Account>) -> (Core, Vec<Effect>) {
        let mut c = core_with(accounts);
        let fx = c.handle(Input::Started {
            width: 1000.0,
            height: 600.0,
        });
        (c, fx)
    }

    fn badges(fx: &[Effect]) -> Vec<DockBadge> {
        fx.iter()
            .filter_map(|e| {
                if let Effect::SetDockBadge(b) = e {
                    Some(*b)
                } else {
                    None
                }
            })
            .collect()
    }

    fn notifies(fx: &[Effect]) -> Vec<&NotificationReq> {
        fx.iter()
            .filter_map(|e| {
                if let Effect::Notify(n) = e {
                    Some(n)
                } else {
                    None
                }
            })
            .collect()
    }

    fn snap(fx: &[Effect]) -> Snapshot {
        fx.iter()
            .rev()
            .find_map(|e| {
                if let Effect::EmitSnapshot(s) = e {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .expect("snapshot")
    }

    #[test]
    fn started_creates_views() {
        let (c, fx) = started(vec![acc(1, "slack", "Slack"), acc(2, "discord", "D")]);
        let content = Bounds {
            x: SIDEBAR,
            y: 0.0,
            width: 1000.0 - SIDEBAR,
            height: 600.0,
        };
        assert_eq!(fx[0], Effect::ApplySystemSettings(AppSettings::default()));
        assert_eq!(
            fx.iter()
                .filter(|e| matches!(e, Effect::ApplySystemSettings(_)))
                .count(),
            1
        );
        assert!(matches!(fx[1], Effect::SetUiBounds(b) if b.width == SIDEBAR && b.height == 600.0));
        let views: Vec<_> = fx
            .iter()
            .filter_map(|e| {
                if let Effect::CreateView {
                    spec,
                    bounds,
                    visible,
                } = e
                {
                    Some((spec.account, *bounds, *visible))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(views, vec![(id(1), content, true), (id(2), content, false)]);
        assert_eq!(badges(&fx), vec![DockBadge::None]);
        assert_eq!(c.snapshot().active, Some(id(1)));
        assert!(matches!(fx[fx.len() - 2], Effect::EmitSnapshot(_)));
        assert!(matches!(fx.last(), Some(Effect::UpdateMenus(_))));
    }

    #[test]
    fn missing_service_gets_no_view() {
        let (_, fx) = started(vec![acc(1, "nope", "x")]);
        assert!(fx.iter().any(|e| matches!(e, Effect::Log(_))));
        assert!(!fx.iter().any(|e| matches!(e, Effect::CreateView { .. })));
    }

    #[test]
    fn add_then_select() {
        let (mut c, _) = started(vec![acc(1, "slack", "Slack")]);
        let fx = c.handle(Input::Ui(UiCommand::AddAccount {
            service: "mastodon".into(),
            label: "  ".into(),
            instance: Some(" https://fosstodon.org/ ".into()),
        }));
        assert_eq!(fx[0], Effect::Hide(id(1)));
        let (spec, visible) = views(&fx)[0];
        assert!(visible);
        assert_eq!(spec.account, id(101));
        assert_eq!(spec.url.as_str(), "https://fosstodon.org/");
        let s = snap(&fx);
        assert_eq!(s.active, Some(id(101)));
        assert_eq!(s.accounts[1].settings.label, "Mastodon");
        assert_eq!(s.accounts[1].instance_label.as_deref(), Some("Instance"));
        assert_eq!(
            s.accounts[1].settings.instance.as_deref(),
            Some("fosstodon.org")
        );
        let fx = c.handle(Input::Ui(UiCommand::Select { account: id(1) }));
        assert_eq!(fx[0], Effect::Hide(id(101)));
        assert!(fx.contains(&Effect::Show(id(1))));
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Persist(p) if p.settings.active == Some(id(1))))
        );
    }

    #[test]
    fn add_unknown_service_logs() {
        let (mut c, _) = started(vec![]);
        let fx = c.handle(Input::Ui(UiCommand::AddAccount {
            service: "zzz".into(),
            label: "a".into(),
            instance: None,
        }));
        assert!(matches!(fx[0], Effect::Log(_)));
        assert!(matches!(fx[1], Effect::EmitSnapshot(_)));
        assert_eq!(fx.len(), 2);
    }

    #[test]
    fn title_unread_and_dock_badge() {
        let (mut c, _) = started(vec![acc(1, "discord", "Discord")]);
        let t = |c: &mut Core, title: &str| {
            c.handle(Input::TitleChanged {
                account: id(1),
                title: title.into(),
            })
        };
        let fx = t(&mut c, "(3) Discord | x");
        assert_eq!(snap(&fx).accounts[0].unread, 3);
        assert_eq!(badges(&fx), vec![DockBadge::Count(3)]);
        assert!(t(&mut c, "(3) Discord | x").is_empty());
        let fx = t(&mut c, "• Discord | x");
        assert_eq!(badges(&fx), vec![DockBadge::Dot]);
        assert!(snap(&fx).accounts[0].indirect);
        let fx = t(&mut c, "Discord");
        assert_eq!(badges(&fx), vec![DockBadge::None]);
    }

    #[test]
    fn muted_not_in_badge() {
        let mut a = acc(1, "discord", "Discord");
        a.muted = true;
        let (mut c, _) = started(vec![a]);
        let fx = c.handle(Input::TitleChanged {
            account: id(1),
            title: "(3) D".into(),
        });
        assert!(badges(&fx).is_empty());
        assert_eq!(snap(&fx).accounts[0].unread, 3);
    }

    #[test]
    fn poll_notifies_with_prefix() {
        let (mut c, _) = started(vec![acc(1, "slack", "KIT"), acc(2, "slack", "Slack")]);
        let inner = r#"{"n":[{"id":7,"title":"Alice","body":"hi"}],"u":null}"#;
        let raw = serde_json::to_string(inner).unwrap();
        let fx = c.handle(Input::PollResult {
            account: id(1),
            raw,
        });
        let n = notifies(&fx);
        assert_eq!(n.len(), 1);
        assert_eq!(
            (n[0].shim_id, n[0].title.as_str(), n[0].body.as_str()),
            (7, "Slack (KIT): Alice", "hi")
        );
        let fx = c.handle(Input::PollResult {
            account: id(2),
            raw: inner.into(),
        });
        assert_eq!(notifies(&fx)[0].title, "Slack: Alice");
    }

    #[test]
    fn poll_muted_or_dnd_silent() {
        let mut a = acc(1, "slack", "Slack");
        a.muted = true;
        let (mut c, _) = started(vec![a, acc(2, "slack", "Slack")]);
        let raw = r#"{"n":[{"id":1,"title":"x"}],"u":null}"#.to_string();
        assert!(
            notifies(&c.handle(Input::PollResult {
                account: id(1),
                raw: raw.clone()
            }))
            .is_empty()
        );
        assert_eq!(
            notifies(&c.handle(Input::PollResult {
                account: id(2),
                raw: raw.clone()
            }))
            .len(),
            1
        );
        c.handle(Input::Ui(UiCommand::SetDnd { enabled: true }));
        assert!(
            notifies(&c.handle(Input::PollResult {
                account: id(2),
                raw
            }))
            .is_empty()
        );
    }

    #[test]
    fn poll_unread_and_bad_json() {
        let mut def = Catalog::parse("p", "id='p'\nname='P'\ncolor='#fff'\nurl='https://p.io'\n[unread]\npoll='1+1'\ntitle_regex='^\\((\\d+)\\)'").unwrap();
        def.id = "p".into();
        let mut cat = Catalog::builtin();
        cat.insert(def);
        let persisted = Persisted {
            accounts: vec![acc(1, "p", "P")],
            ..Default::default()
        };
        let mut c = Core::new(cat, persisted, Box::new(|| Uuid::from_u128(999)));
        c.handle(Input::Started {
            width: 800.0,
            height: 600.0,
        });
        let fx = c.handle(Input::PollResult {
            account: id(1),
            raw: r#"{"n":null,"u":4}"#.into(),
        });
        assert_eq!(badges(&fx), vec![DockBadge::Count(4)]);
        // poll wins over titles
        assert!(
            c.handle(Input::TitleChanged {
                account: id(1),
                title: "(9) P".into()
            })
            .is_empty()
        );
        let fx = c.handle(Input::PollResult {
            account: id(1),
            raw: r#"{"n":[],"u":{"direct":0,"indirect":true}}"#.into(),
        });
        assert_eq!(badges(&fx), vec![DockBadge::Dot]);
        let fx = c.handle(Input::PollResult {
            account: id(1),
            raw: "not json".into(),
        });
        assert!(matches!(fx.as_slice(), [Effect::Log(_)]));
        let fx = c.handle(Input::Tick { now_ms: 1000 });
        match &fx[0] {
            Effect::Poll { script, .. } => assert!(script.contains("u=(1+1)")),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn tick_script_exact_and_unknown_account() {
        let (mut c, _) = started(vec![acc(1, "slack", "Slack")]);
        let fx = c.handle(Input::Tick { now_ms: 1000 });
        assert_eq!(
            fx,
            vec![Effect::Poll {
                account: id(1),
                script: "(()=>{const m=window.__msgr;let u=null;try{u=(null)}catch(e){}return JSON.stringify({n:m?m.drain():[],u:u});})()".into()
            }]
        );
        assert!(matches!(
            c.handle(Input::TitleChanged {
                account: id(99),
                title: "x".into()
            })
            .as_slice(),
            [Effect::Log(_)]
        ));
        assert!(
            core_with(vec![acc(1, "slack", "S")])
                .handle(Input::Tick { now_ms: 1000 })
                .is_empty()
        );
    }

    #[test]
    fn notification_click() {
        let (mut c, _) = started(vec![acc(1, "slack", "Slack"), acc(2, "slack", "B")]);
        let fx = c.handle(Input::NotificationClicked {
            account: id(2),
            shim_id: 5,
        });
        let sel = c.handle(Input::Ui(UiCommand::Select { account: id(1) }));
        assert!(fx.contains(&Effect::Hide(id(1))) && fx.contains(&Effect::Show(id(2))));
        assert!(!sel.is_empty());
        let n = fx.len();
        assert_eq!(fx[n - 2], Effect::FocusWindow);
        assert_eq!(
            fx[n - 1],
            Effect::Eval {
                account: id(2),
                script: "window.__msgr&&window.__msgr.click(5)".into()
            }
        );
    }

    #[test]
    fn remove_account_picks_next() {
        let (mut c, _) = started(vec![
            acc(1, "slack", "A"),
            acc(2, "slack", "B"),
            acc(3, "slack", "C"),
        ]);
        c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
        let fx = c.handle(Input::Ui(UiCommand::RemoveAccount { account: id(2) }));
        assert_eq!(fx[0], Effect::Destroy(id(2)));
        assert_eq!(
            fx[1],
            Effect::DeleteProfile {
                store_id: *id(2).as_bytes()
            }
        );
        assert!(fx.contains(&Effect::Show(id(3))));
        assert_eq!(snap(&fx).active, Some(id(3)));
        let fx = c.handle(Input::Ui(UiCommand::RemoveAccount { account: id(3) }));
        assert_eq!(snap(&fx).active, Some(id(1)));
        let fx = c.handle(Input::Ui(UiCommand::RemoveAccount { account: id(1) }));
        assert_eq!(snap(&fx).active, None);
    }

    #[test]
    fn move_clamps() {
        let (mut c, _) = started(vec![
            acc(1, "slack", "A"),
            acc(2, "slack", "B"),
            acc(3, "slack", "C"),
        ]);
        let fx = c.handle(Input::Ui(UiCommand::Move {
            account: id(1),
            delta: 10,
        }));
        let order: Vec<_> = snap(&fx).accounts.iter().map(|a| a.id).collect();
        assert_eq!(order, vec![id(2), id(3), id(1)]);
        let fx = c.handle(Input::Ui(UiCommand::Move {
            account: id(1),
            delta: -10,
        }));
        assert_eq!(snap(&fx).accounts[0].id, id(1));
    }

    #[test]
    fn dialog_bounds() {
        let (mut c, _) = started(vec![acc(1, "slack", "A")]);
        let fx = c.handle(Input::Ui(UiCommand::SetDialog { open: true }));
        assert!(matches!(fx[0], Effect::SetUiBounds(b) if b.width == 1000.0));
        assert_eq!(fx[1], Effect::Hide(id(1)));
        assert!(snap(&fx).dialog_open);
        let fx = c.handle(Input::Ui(UiCommand::SetDialog { open: false }));
        assert!(matches!(fx[0], Effect::SetUiBounds(b) if b.width == SIDEBAR));
        assert!(matches!(fx[1], Effect::SetBounds { .. }));
        assert_eq!(fx[2], Effect::Show(id(1)));
        let fx = c.handle(Input::Ui(UiCommand::SetDialog { open: false }));
        assert!(matches!(fx.as_slice(), [Effect::EmitSnapshot(_)]));
    }

    #[test]
    fn icon_size_sets_sidebar_width() {
        assert_eq!(IconSize::default().sidebar_width(), SIDEBAR);
        let (mut c, _) = started(vec![acc(1, "slack", "A")]);
        let large = AppSettings {
            icon_size: IconSize::Large,
            ..AppSettings::default()
        };
        let fx = c.handle(Input::Ui(UiCommand::UpdateSettings {
            settings: large.clone(),
        }));
        let content = Bounds {
            x: 72.0,
            y: 0.0,
            width: 1000.0 - 72.0,
            height: 600.0,
        };
        assert!(fx.contains(&Effect::SetUiBounds(Bounds {
            x: 0.0,
            y: 0.0,
            width: 72.0,
            height: 600.0
        })));
        assert!(fx.contains(&Effect::SetBounds {
            account: id(1),
            bounds: content
        }));
        // Unchanged icon size: no layout effects.
        let fx = c.handle(Input::Ui(UiCommand::UpdateSettings { settings: large }));
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::SetUiBounds(_) | Effect::SetBounds { .. }))
        );
        // Resizes and newly shown views use the new width.
        let fx = c.handle(Input::WindowResized {
            width: 800.0,
            height: 500.0,
        });
        assert!(fx.contains(&Effect::SetBounds {
            account: id(1),
            bounds: Bounds {
                x: 72.0,
                y: 0.0,
                width: 728.0,
                height: 500.0
            }
        }));
    }

    #[test]
    fn icon_size_change_with_open_dialog_keeps_views_covered() {
        let (mut c, _) = started(vec![acc(1, "slack", "A")]);
        c.handle(Input::Ui(UiCommand::SetDialog { open: true }));
        let medium = AppSettings {
            icon_size: IconSize::Medium,
            ..AppSettings::default()
        };
        let fx = c.handle(Input::Ui(UiCommand::UpdateSettings { settings: medium }));
        assert!(fx.contains(&Effect::SetUiBounds(Bounds {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 600.0
        })));
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::SetBounds { .. } | Effect::Show(_)))
        );
        let fx = c.handle(Input::Ui(UiCommand::SetDialog { open: false }));
        assert_eq!(
            fx[0],
            Effect::SetUiBounds(Bounds {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 600.0
            })
        );
        assert_eq!(
            fx[1],
            Effect::SetBounds {
                account: id(1),
                bounds: Bounds {
                    x: 64.0,
                    y: 0.0,
                    width: 936.0,
                    height: 600.0
                }
            }
        );
    }

    #[test]
    fn resize_and_misc() {
        let (mut c, _) = started(vec![acc(1, "slack", "A")]);
        let fx = c.handle(Input::WindowResized {
            width: 50.0,
            height: 300.0,
        });
        assert!(
            matches!(fx[1], Effect::SetBounds { bounds, .. } if bounds.width == 0.0 && bounds.height == 300.0)
        );
        assert!(
            c.handle(Input::PageLoaded {
                account: id(99),
                url: "x".into()
            })
            .is_empty()
        );
        assert_eq!(
            c.handle(Input::ExternalLink {
                account: id(1),
                url: "https://u.org/".into()
            }),
            vec![Effect::OpenExternal {
                url: "https://u.org/".into(),
                browser: None
            }]
        );
    }

    fn raw_notification(body: &str) -> String {
        format!(r#"{{"n":[{{"id":1,"title":"Alice","body":"{body}"}}],"u":null}}"#)
    }

    #[test]
    fn add_from_open_dialog_keeps_dialog_and_active() {
        let (mut c, _) = started(vec![acc(1, "slack", "A")]);
        c.handle(Input::Ui(UiCommand::SetDialog { open: true }));
        let fx = c.handle(Input::Ui(UiCommand::AddAccount {
            service: "discord".into(),
            label: "D".into(),
            instance: None,
        }));
        assert_eq!(views(&fx).len(), 1);
        assert!(
            !views(&fx)[0].1,
            "new view stays hidden while the dialog is open"
        );
        assert!(!fx.iter().any(|e| matches!(
            e,
            Effect::SetUiBounds(_) | Effect::Show(_) | Effect::Hide(_)
        )));
        let s = snap(&fx);
        assert!(s.dialog_open);
        assert_eq!(s.active, Some(id(1)));
        assert_eq!(s.accounts.len(), 2);
    }

    #[test]
    fn add_first_account_from_dialog_becomes_active_on_close() {
        let (mut c, _) = started(vec![]);
        c.handle(Input::Ui(UiCommand::SetDialog { open: true }));
        let fx = c.handle(Input::Ui(UiCommand::AddAccount {
            service: "discord".into(),
            label: "".into(),
            instance: None,
        }));
        let new = snap(&fx).accounts[0].id;
        assert_eq!(snap(&fx).active, Some(new));
        let fx = c.handle(Input::Ui(UiCommand::SetDialog { open: false }));
        assert!(fx.contains(&Effect::Show(new)));
    }

    #[test]
    fn element_clears_stale_session_lock_before_page_scripts() {
        let def = Catalog::builtin().get("element").cloned().unwrap();
        assert!(
            def.clear_local_storage
                .iter()
                .any(|k| k == "react_sdk_session_lock_ping"),
            "{def:?}"
        );
        let (_, fx) = started(vec![acc(1, "element", "E")]);
        let script = &views(&fx)[0].0.init_script;
        assert!(
            script.starts_with("(()=>{try{for(const k of [\"react_sdk_session_lock_ping\""),
            "{script:.120}"
        );
        assert!(script.contains(crate::NOTIFY_SHIM));
    }

    fn views(fx: &[Effect]) -> Vec<(&ViewSpec, bool)> {
        fx.iter()
            .filter_map(|e| {
                if let Effect::CreateView { spec, visible, .. } = e {
                    Some((spec, *visible))
                } else {
                    None
                }
            })
            .collect()
    }

    fn update_account(c: &mut Core, n: u128, f: impl FnOnce(&mut AccountSettings)) -> Vec<Effect> {
        let mut settings = c.account(id(n)).unwrap().settings();
        f(&mut settings);
        c.handle(Input::Ui(UiCommand::UpdateAccount {
            account: id(n),
            settings,
        }))
    }

    #[test]
    fn snapshot_carries_settings_app_info_and_account_settings() {
        let mut a = acc(1, "mastodon", "Fedi");
        a.instance = Some("fosstodon.org".into());
        a.show_badge = false;
        let mut c = core_with(vec![a, acc(2, "slack", "Slack")]);
        let info = AppInfo {
            version: "1.2.3".into(),
            config_dir: "/cfg".into(),
            services_dir: "/cfg/services".into(),
            log_file: "/log/m.log".into(),
        };
        c.set_app_info(info.clone());
        let s = c.snapshot();
        assert_eq!(s.app, info);
        assert_eq!(s.settings, AppSettings::default());
        assert_eq!(s.accounts[0].instance_label.as_deref(), Some("Instance"));
        assert_eq!(s.accounts[1].instance_label, None);
        let expected = AccountSettings {
            label: "Fedi".into(),
            instance: Some("fosstodon.org".into()),
            muted: false,
            show_badge: false,
            in_dock_badge: true,
            user_agent: None,
            enabled: true,
            link_target: None,
            custom_icon: None,
            simple_icon: None,
            zoom: 1.0,
            hibernate_after_min: None,
            custom_css: None,
            allow_media: None,
            app_hosts: vec![],
        };
        assert_eq!(s.accounts[0].settings, expected);
    }

    #[test]
    fn set_muted_and_dnd_map_onto_settings() {
        let (mut c, _) = started(vec![acc(1, "slack", "Slack")]);
        let fx = c.handle(Input::Ui(UiCommand::SetMuted {
            account: id(1),
            muted: true,
        }));
        assert!(snap(&fx).accounts[0].settings.muted);
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Persist(p) if p.accounts[0].muted))
        );
        let fx = c.handle(Input::Ui(UiCommand::SetDnd { enabled: true }));
        assert!(snap(&fx).settings.dnd);
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Persist(p) if p.settings.app.dnd))
        );
    }

    #[test]
    fn update_settings_persists_applies_and_emits() {
        let (mut c, _) = started(vec![acc(1, "discord", "Discord")]);
        c.handle(Input::TitleChanged {
            account: id(1),
            title: "• Discord".into(),
        });
        let settings = AppSettings {
            launch_at_login: true,
            dock_badge: DockBadgeMode::CountOnly,
            browser: Some("  Firefox ".into()),
            ..AppSettings::default()
        };
        let fx = c.handle(Input::Ui(UiCommand::UpdateSettings {
            settings: settings.clone(),
        }));
        let stored = AppSettings {
            browser: Some("Firefox".into()),
            ..settings
        };
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Persist(p) if p.settings.app == stored))
        );
        assert!(fx.contains(&Effect::ApplySystemSettings(stored.clone())));
        assert_eq!(badges(&fx), vec![DockBadge::None]);
        assert_eq!(snap(&fx).settings, stored);
        let fx = c.handle(Input::ExternalLink {
            account: id(1),
            url: "https://x.org".into(),
        });
        assert_eq!(
            fx,
            vec![Effect::OpenExternal {
                url: "https://x.org".into(),
                browser: Some("Firefox".into())
            }]
        );
        let blank = AppSettings {
            browser: Some(" ".into()),
            ..AppSettings::default()
        };
        c.handle(Input::Ui(UiCommand::UpdateSettings { settings: blank }));
        assert_eq!(c.snapshot().settings.browser, None);
    }

    #[test]
    fn dock_badge_modes() {
        let (mut c, _) = started(vec![acc(1, "discord", "A"), acc(2, "discord", "B")]);
        let mode = |c: &mut Core, dock_badge| {
            let settings = AppSettings {
                dock_badge,
                ..AppSettings::default()
            };
            badges(&c.handle(Input::Ui(UiCommand::UpdateSettings { settings })))
        };
        c.handle(Input::TitleChanged {
            account: id(1),
            title: "• A".into(),
        });
        assert_eq!(
            mode(&mut c, DockBadgeMode::CountOnly),
            vec![DockBadge::None]
        );
        assert_eq!(mode(&mut c, DockBadgeMode::All), vec![DockBadge::Dot]);
        let fx = c.handle(Input::TitleChanged {
            account: id(2),
            title: "(2) B".into(),
        });
        assert_eq!(badges(&fx), vec![DockBadge::Count(2)]);
        assert_eq!(mode(&mut c, DockBadgeMode::Off), vec![DockBadge::None]);
        let fx = c.handle(Input::TitleChanged {
            account: id(2),
            title: "(5) B".into(),
        });
        assert!(badges(&fx).is_empty());
        assert_eq!(snap(&fx).accounts[1].unread, 5);
        assert_eq!(
            mode(&mut c, DockBadgeMode::CountOnly),
            vec![DockBadge::Count(5)]
        );
    }

    #[test]
    fn dock_badge_respects_in_dock_badge() {
        let mut a = acc(1, "discord", "A");
        a.in_dock_badge = false;
        let (mut c, _) = started(vec![a, acc(2, "discord", "B")]);
        let fx = c.handle(Input::TitleChanged {
            account: id(1),
            title: "(3) A".into(),
        });
        assert!(badges(&fx).is_empty());
        assert_eq!(snap(&fx).accounts[0].unread, 3);
        let fx = update_account(&mut c, 1, |s| s.in_dock_badge = true);
        assert_eq!(badges(&fx), vec![DockBadge::Count(3)]);
        let fx = update_account(&mut c, 1, |s| s.muted = true);
        assert_eq!(badges(&fx), vec![DockBadge::None]);
    }

    #[test]
    fn show_badge_is_ui_only() {
        let (mut c, _) = started(vec![acc(1, "discord", "A")]);
        let fx = update_account(&mut c, 1, |s| s.show_badge = false);
        assert!(!snap(&fx).accounts[0].settings.show_badge);
        let fx = c.handle(Input::TitleChanged {
            account: id(1),
            title: "(3) A".into(),
        });
        assert_eq!(badges(&fx), vec![DockBadge::Count(3)]);
        assert_eq!(snap(&fx).accounts[0].unread, 3);
    }

    #[test]
    fn notification_preview_and_sound() {
        let (mut c, _) = started(vec![acc(1, "slack", "Slack")]);
        let raw = raw_notification("secret");
        let fx = c.handle(Input::PollResult {
            account: id(1),
            raw: raw.clone(),
        });
        assert_eq!(
            (notifies(&fx)[0].body.as_str(), notifies(&fx)[0].sound),
            ("secret", true)
        );
        let settings = AppSettings {
            notification_preview: false,
            notification_sound: false,
            ..AppSettings::default()
        };
        c.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
        let fx = c.handle(Input::PollResult {
            account: id(1),
            raw,
        });
        let n = notifies(&fx);
        assert_eq!(
            (n[0].title.as_str(), n[0].body.as_str(), n[0].sound),
            ("Slack: Alice", HIDDEN_PREVIEW_BODY, false)
        );
    }

    #[test]
    fn update_account_simple_fields_no_recreate() {
        let (mut c, _) = started(vec![acc(1, "slack", "Slack")]);
        let fx = update_account(&mut c, 1, |s| {
            s.label = "  Work ".into();
            s.muted = true;
        });
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::Destroy(_) | Effect::CreateView { .. }))
        );
        assert!(fx.iter().any(|e| matches!(e, Effect::Persist(p) if p.accounts[0].label == "Work" && p.accounts[0].muted)));
        let s = snap(&fx);
        assert_eq!(
            (
                s.accounts[0].settings.label.as_str(),
                s.accounts[0].initials.as_str()
            ),
            ("Work", "Wo")
        );
        let fx = update_account(&mut c, 1, |s| s.label = " ".into());
        assert_eq!(snap(&fx).accounts[0].settings.label, "Slack");
        let fx = c.handle(Input::Ui(UiCommand::UpdateAccount {
            account: id(9),
            settings: c.account(id(1)).unwrap().settings(),
        }));
        assert!(matches!(fx.as_slice(), [Effect::Log(_)]));
    }

    #[test]
    fn update_account_instance_recreates_view() {
        let mut a = acc(1, "mastodon", "M");
        a.instance = Some("mastodon.social".into());
        let (mut c, _) = started(vec![acc(2, "slack", "S"), a]);
        // inactive account -> recreated hidden
        let fx = update_account(&mut c, 1, |s| {
            s.instance = Some(" https://fosstodon.org/ ".into())
        });
        assert_eq!(fx[0], Effect::Destroy(id(1)));
        let v = views(&fx);
        assert_eq!(v.len(), 1);
        assert_eq!(
            (v[0].0.url.as_str(), v[0].0.store_id, v[0].1),
            ("https://fosstodon.org/", *id(1).as_bytes(), false)
        );
        assert_eq!(v[0].0.internal_hosts, vec!["fosstodon.org".to_string()]);
        assert!(fx.iter().any(|e| matches!(e, Effect::Persist(p) if p.accounts[1].instance.as_deref() == Some("fosstodon.org"))));
        // same instance again -> nothing recreated
        let fx = update_account(&mut c, 1, |s| s.instance = Some("fosstodon.org/".into()));
        assert!(views(&fx).is_empty());
        // active account -> visible, unless the dialog is open
        c.handle(Input::Ui(UiCommand::Select { account: id(1) }));
        let fx = update_account(&mut c, 1, |s| s.instance = Some("a.social".into()));
        assert!(views(&fx)[0].1);
        c.handle(Input::Ui(UiCommand::SetDialog { open: true }));
        let fx = update_account(&mut c, 1, |s| s.instance = Some("b.social".into()));
        assert!(!views(&fx)[0].1);
    }

    #[test]
    fn update_account_invalid_instance_changes_nothing() {
        let mut a = acc(1, "mastodon", "M");
        a.instance = Some("mastodon.social".into());
        let (mut c, _) = started(vec![a]);
        let before = c.snapshot();
        let fx = update_account(&mut c, 1, |s| {
            s.label = "New".into();
            s.instance = Some("bad host/x y".into());
        });
        assert!(
            matches!(fx.as_slice(), [Effect::Log(_), Effect::EmitSnapshot(_)]),
            "{fx:?}"
        );
        assert_eq!(c.snapshot(), before);
    }

    #[test]
    fn user_agent_override() {
        let (mut c, _) = started(vec![acc(1, "slack", "S")]);
        let service_ua = c.view_spec(c.account(id(1)).unwrap()).unwrap().user_agent;
        let ua = |c: &mut Core, value: Option<&str>| {
            let fx = update_account(c, 1, |s| s.user_agent = value.map(String::from));
            assert_eq!(fx[0], Effect::Destroy(id(1)));
            views(&fx)[0].0.user_agent.clone()
        };
        assert_eq!(
            ua(&mut c, Some("chrome")).as_deref(),
            Some(crate::catalog::UA_CHROME)
        );
        assert_eq!(ua(&mut c, Some("default")), None);
        assert_eq!(ua(&mut c, Some(" Foo/1 ")).as_deref(), Some("Foo/1"));
        assert_eq!(ua(&mut c, Some("  ")), service_ua);
        assert_eq!(c.snapshot().accounts[0].settings.user_agent, None);
    }

    #[test]
    fn update_account_before_start_does_not_recreate() {
        let mut c = core_with(vec![acc(1, "slack", "S")]);
        let fx = update_account(&mut c, 1, |s| s.user_agent = Some("chrome".into()));
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::Destroy(_) | Effect::CreateView { .. }))
        );
        let fx = c.handle(Input::Started {
            width: 800.0,
            height: 600.0,
        });
        assert_eq!(
            views(&fx)[0].0.user_agent.as_deref(),
            Some(crate::catalog::UA_CHROME)
        );
    }

    #[test]
    fn initials_rules() {
        assert_eq!(initials("my work"), "MW");
        assert_eq!(initials("slack"), "Sl");
        assert_eq!(initials("a b c"), "AB");
    }

    #[test]
    fn is_internal() {
        let spec = ViewSpec {
            account: id(1),
            url: url::Url::parse("https://a.com").unwrap(),
            store_id: [0; 16],
            user_agent: None,
            init_script: String::new(),
            internal_hosts: vec!["*.slack.com".into(), "x.org".into()],
        };
        let u = |s: &str| url::Url::parse(s).unwrap();
        assert!(spec.is_internal(&u("about:blank")));
        assert!(spec.is_internal(&u("blob:https://evil.com/1")));
        assert!(spec.is_internal(&u("https://app.slack.com/x")));
        assert!(spec.is_internal(&u("https://SLACK.com/")));
        assert!(!spec.is_internal(&u("https://evilslack.com/")));
        assert!(!spec.is_internal(&u("https://sub.x.org/")));
    }

    // ---- feature batch 2 ----

    fn has(fx: &[Effect], e: &Effect) -> bool {
        fx.contains(e)
    }

    fn destroys(fx: &[Effect]) -> Vec<Uuid> {
        fx.iter()
            .filter_map(|e| {
                if let Effect::Destroy(a) = e {
                    Some(*a)
                } else {
                    None
                }
            })
            .collect()
    }

    fn polls(fx: &[Effect]) -> Vec<Uuid> {
        fx.iter()
            .filter_map(|e| {
                if let Effect::Poll { account, .. } = e {
                    Some(*account)
                } else {
                    None
                }
            })
            .collect()
    }

    fn logs(fx: &[Effect]) -> Vec<&str> {
        fx.iter()
            .filter_map(|e| {
                if let Effect::Log(m) = e {
                    Some(m.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    fn menus(fx: &[Effect]) -> Vec<&MenuModel> {
        fx.iter()
            .filter_map(|e| {
                if let Effect::UpdateMenus(m) = e {
                    Some(m)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Ticks every 10 s from `from` to `to` (inclusive), collecting all effects.
    fn tick_range(c: &mut Core, from: u64, to: u64) -> Vec<Effect> {
        let mut fx = vec![];
        let mut t = from;
        while t <= to {
            fx.extend(c.handle(Input::Tick { now_ms: t }));
            t += 10_000;
        }
        fx
    }

    fn custom(name: &str, url: &str) -> CustomService {
        CustomService {
            id: String::new(),
            name: name.into(),
            url: url.into(),
            color: "#123456".into(),
            instance_label: None,
            title_regex: None,
            user_agent: None,
        }
    }

    fn pol(link_target: LinkTarget, allow_media: bool) -> ViewPolicy {
        ViewPolicy {
            link_target,
            allow_media,
            app_hosts: vec![],
        }
    }

    fn policies(fx: &[Effect]) -> Vec<(Uuid, ViewPolicy)> {
        fx.iter()
            .filter_map(|e| {
                if let Effect::SetViewPolicy { account, policy } = e {
                    Some((*account, policy.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn app_hosts_normalised_and_policy_updated_live() {
        let (mut c, _) = started(vec![acc(1, "slack", "A")]);
        let fx = update_account(&mut c, 1, |s| {
            s.app_hosts = vec![
                " HTTPS://IdP.Example.org:443/login ".into(),
                "idp.example.org".into(),
                "*.microsoftonline.com".into(),
                "no good".into(),
            ]
        });
        let want = vec![
            "idp.example.org".to_string(),
            "*.microsoftonline.com".into(),
        ];
        let got = policies(&fx);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1.app_hosts, want);
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::Destroy(_) | Effect::CreateView { .. }))
        );
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Log(m) if m.contains("no good")))
        );
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Persist(p) if p.accounts[0].app_hosts == want))
        );
        assert_eq!(snap(&fx).accounts[0].settings.app_hosts, want);
        // same list again -> no policy effect
        let fx = update_account(&mut c, 1, |s| s.app_hosts = want.clone());
        assert!(policies(&fx).is_empty());
    }

    #[test]
    fn view_policy_app_host_matching() {
        let p = ViewPolicy {
            app_hosts: vec!["idp.example.org".into(), "*.ms.com".into()],
            ..pol(LinkTarget::SystemBrowser, false)
        };
        assert!(p.is_app_host("idp.example.org"));
        assert!(p.is_app_host("IDP.example.org"));
        assert!(!p.is_app_host("x.idp.example.org"));
        assert!(p.is_app_host("ms.com"));
        assert!(p.is_app_host("login.ms.com"));
        assert!(!p.is_app_host("notms.com"));
        assert!(!pol(LinkTarget::AppWindow, false).is_app_host("ms.com"));
    }

    #[test]
    fn view_policy_emitted_on_create_and_change() {
        let mut b = acc(2, "slack", "B");
        b.link_target = Some(LinkTarget::SystemBrowser);
        let (mut c, fx) = started(vec![acc(1, "slack", "A"), b]);
        let pos = |fx: &[Effect], e: &Effect| fx.iter().position(|x| x == e);
        let set1 = Effect::SetViewPolicy {
            account: id(1),
            policy: pol(LinkTarget::SystemBrowser, true),
        };
        let created = fx
            .iter()
            .position(|e| matches!(e, Effect::CreateView { spec, .. } if spec.account == id(1)));
        assert!(
            pos(&fx, &set1).unwrap() < created.unwrap(),
            "policy is set before the view exists"
        );
        assert!(has(
            &fx,
            &Effect::SetZoom {
                account: id(1),
                zoom: 1.0
            }
        ));

        let settings = AppSettings {
            link_target: LinkTarget::AppWindow,
            ..AppSettings::default()
        };
        let fx = c.handle(Input::Ui(UiCommand::UpdateSettings {
            settings: settings.clone(),
        }));
        assert_eq!(
            policies(&fx),
            vec![(id(1), pol(LinkTarget::AppWindow, true))]
        );
        // unchanged global -> nothing
        let fx = c.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
        assert!(policies(&fx).is_empty());

        // account override equal to the effective target -> nothing; different -> effect
        let fx = update_account(&mut c, 1, |s| s.link_target = Some(LinkTarget::AppWindow));
        assert!(policies(&fx).is_empty());
        let fx = update_account(&mut c, 2, |s| s.link_target = None);
        assert_eq!(
            policies(&fx),
            vec![(id(2), pol(LinkTarget::AppWindow, true))]
        );
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::Destroy(_) | Effect::CreateView { .. }))
        );
        assert_eq!(c.account(id(2)).unwrap().link_target, None);
    }

    #[test]
    fn allow_media_from_service_and_override() {
        let (mut c, fx) = started(vec![acc(1, "slack", "S"), acc(2, "linkedin", "L")]);
        let p = policies(&fx);
        assert_eq!(
            p,
            vec![
                (id(1), pol(LinkTarget::SystemBrowser, true)),
                (id(2), pol(LinkTarget::SystemBrowser, false))
            ]
        );
        let s = c.snapshot();
        let info = |id: &str| s.services.iter().find(|x| x.id == id).unwrap().allow_media;
        assert!(info("slack") && info("rainbow") && !info("linkedin"));
        // override without recreating the view; persisted
        let fx = update_account(&mut c, 2, |s| s.allow_media = Some(true));
        assert_eq!(
            policies(&fx),
            vec![(id(2), pol(LinkTarget::SystemBrowser, true))]
        );
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::Destroy(_) | Effect::CreateView { .. }))
        );
        assert!(
            fx.iter().any(
                |e| matches!(e, Effect::Persist(p) if p.accounts[1].allow_media == Some(true))
            )
        );
        assert_eq!(snap(&fx).accounts[1].settings.allow_media, Some(true));
        let fx = update_account(&mut c, 1, |s| s.allow_media = Some(false));
        assert_eq!(
            policies(&fx),
            vec![(id(1), pol(LinkTarget::SystemBrowser, false))]
        );
        // explicit value equal to the service default -> no change
        let fx = update_account(&mut c, 2, |s| s.allow_media = None);
        assert_eq!(
            policies(&fx),
            vec![(id(2), pol(LinkTarget::SystemBrowser, false))]
        );
        let fx = update_account(&mut c, 2, |s| s.allow_media = Some(false));
        assert!(policies(&fx).is_empty());
        // hibernated: applied on the next create
        c.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
        assert!(policies(&update_account(&mut c, 2, |s| s.allow_media = Some(true))).is_empty());
        let fx = c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
        assert_eq!(
            policies(&fx),
            vec![(id(2), pol(LinkTarget::SystemBrowser, true))]
        );
        // custom services default to no media
        c.handle(Input::Ui(UiCommand::AddCustomService {
            service: custom("Site", "https://site.org"),
        }));
        let fx = c.handle(Input::Ui(UiCommand::AddAccount {
            service: "custom-site".into(),
            label: "".into(),
            instance: None,
        }));
        assert_eq!(
            policies(&fx),
            vec![(id(101), pol(LinkTarget::SystemBrowser, false))]
        );
        assert!(
            !snap(&fx)
                .services
                .iter()
                .find(|x| x.id == "custom-site")
                .unwrap()
                .allow_media
        );
    }

    #[test]
    fn open_project_page_opens_the_repository() {
        let (mut c, _) = started(vec![]);
        assert_eq!(
            c.handle(Input::Ui(UiCommand::OpenProjectPage)),
            vec![Effect::OpenExternal {
                url: protocol::PROJECT_URL.into(),
                browser: None
            }]
        );
    }

    #[test]
    fn external_links_only_web_and_mail() {
        let (mut c, _) = started(vec![acc(1, "slack", "S")]);
        let open = |c: &mut Core, url: &str| {
            c.handle(Input::ExternalLink {
                account: id(1),
                url: url.into(),
            })
        };
        assert_eq!(
            open(&mut c, "mailto:a@b.org"),
            vec![Effect::OpenExternal {
                url: "mailto:a@b.org".into(),
                browser: None
            }]
        );
        assert!(matches!(
            open(&mut c, "https://x.org").as_slice(),
            [Effect::OpenExternal { .. }]
        ));
        for bad in [
            "file:///etc/hosts",
            "x-apple.systempreferences:com.apple.preference.security",
            "slack://channel",
            "javascript:1",
        ] {
            assert!(
                matches!(open(&mut c, bad).as_slice(), [Effect::Log(_)]),
                "{bad}"
            );
        }
    }

    #[test]
    fn hibernate_after_idle_time() {
        let mut b = acc(2, "slack", "B");
        b.hibernate_after_min = Some(1);
        let mut a = acc(1, "slack", "A");
        a.hibernate_after_min = Some(1);
        let (mut c, _) = started(vec![a, b]);
        let fx = tick_range(&mut c, 0, 50_000);
        assert!(destroys(&fx).is_empty());
        let fx = c.handle(Input::Tick { now_ms: 60_000 });
        // the active account (1) is never hibernated
        assert_eq!(destroys(&fx), vec![id(2)]);
        assert_eq!(polls(&fx), vec![id(1)]);
        let s = snap(&fx);
        assert!(!s.accounts[0].hibernated && s.accounts[1].hibernated);
        // a hibernated account is not polled and not unloaded twice
        let fx = tick_range(&mut c, 70_000, 200_000);
        assert!(destroys(&fx).is_empty());
        assert!(!polls(&fx).contains(&id(2)));

        // selecting it loads it again (visible); the account left becomes idle from now on
        let fx = c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
        assert_eq!(fx[0], Effect::Hide(id(1)));
        assert_eq!(
            views(&fx)
                .iter()
                .map(|(s, v)| (s.account, *v))
                .collect::<Vec<_>>(),
            vec![(id(2), true)]
        );
        assert!(!snap(&fx).accounts[1].hibernated);
        // left at the last tick (200 s) -> unloaded one minute later
        let fx = tick_range(&mut c, 210_000, 250_000);
        assert!(destroys(&fx).is_empty());
        let fx = c.handle(Input::Tick { now_ms: 260_000 });
        assert_eq!(destroys(&fx), vec![id(1)]);
    }

    fn set_global_hibernate(c: &mut Core, m: Option<u32>) {
        let mut settings = c.settings().clone();
        settings.hibernate_after_min = m;
        c.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
    }

    #[test]
    fn global_hibernate_applies_to_accounts_without_override() {
        let (mut c, _) = started(vec![acc(1, "slack", "A"), acc(2, "slack", "B")]);
        tick_range(&mut c, 0, 50_000);
        // changing the setting unloads nothing by itself
        set_global_hibernate(&mut c, Some(1));
        assert!(destroys(&c.handle(Input::Tick { now_ms: 55_000 })).is_empty());
        // measured from the last activity of the account
        let fx = c.handle(Input::Tick { now_ms: 60_000 });
        assert_eq!(destroys(&fx), vec![id(2)]);
    }

    #[test]
    fn account_override_beats_global_hibernate() {
        let mut b = acc(2, "slack", "B");
        b.hibernate_after_min = Some(2);
        let (mut c, _) = started(vec![acc(1, "slack", "A"), b]);
        set_global_hibernate(&mut c, Some(1));
        tick_range(&mut c, 0, 60_000);
        assert!(destroys(&c.handle(Input::Tick { now_ms: 60_000 })).is_empty());
        let fx = c.handle(Input::Tick { now_ms: 120_000 });
        assert_eq!(destroys(&fx), vec![id(2)]);
    }

    #[test]
    fn zero_means_never_even_with_global_hibernate() {
        let mut b = acc(2, "slack", "B");
        b.hibernate_after_min = Some(0);
        let (mut c, _) = started(vec![acc(1, "slack", "A"), b]);
        set_global_hibernate(&mut c, Some(1));
        let fx = tick_range(&mut c, 0, 600_000);
        assert!(destroys(&fx).is_empty());
    }

    #[test]
    fn active_account_is_never_hibernated_by_global() {
        let (mut c, _) = started(vec![acc(1, "slack", "A")]);
        set_global_hibernate(&mut c, Some(1));
        let fx = tick_range(&mut c, 0, 600_000);
        assert!(destroys(&fx).is_empty());
    }

    #[test]
    fn old_persisted_account_json_loads() {
        let mut v = serde_json::to_value(acc(1, "slack", "A")).unwrap();
        v.as_object_mut().unwrap().remove("hibernate_after_min");
        let a: Account = serde_json::from_value(v).unwrap();
        assert_eq!(a.hibernate_after_min, None);
        let old: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.hibernate_after_min, None);
    }

    #[test]
    fn hibernate_command() {
        let (mut c, _) = started(vec![acc(1, "slack", "A"), acc(2, "slack", "B")]);
        let fx = c.handle(Input::Ui(UiCommand::Hibernate { account: id(1) }));
        assert!(
            matches!(fx.as_slice(), [Effect::Log(_)]),
            "active account: only logged"
        );
        let fx = c.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
        assert_eq!(fx[0], Effect::Destroy(id(2)));
        assert!(snap(&fx).accounts[1].hibernated);
        let fx = c.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
        assert!(destroys(&fx).is_empty());
        assert_eq!(polls(&c.handle(Input::Tick { now_ms: 1 })), vec![id(1)]);
        // commands on an unloaded view are logged, not sent to the host
        let fx = c.handle(Input::Ui(UiCommand::Reload { account: id(2) }));
        assert!(matches!(fx.as_slice(), [Effect::Log(_)]));
        let fx = c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
        assert_eq!(views(&fx).len(), 1);
        assert!(views(&fx)[0].1);
        assert!(matches!(
            c.handle(Input::Ui(UiCommand::Hibernate { account: id(9) }))
                .as_slice(),
            [Effect::Log(_)]
        ));
    }

    #[test]
    fn wake_detection_reloads_loaded_views() {
        let (mut c, _) = started(vec![
            acc(1, "slack", "A"),
            acc(2, "slack", "B"),
            acc(3, "slack", "C"),
        ]);
        c.handle(Input::Ui(UiCommand::Hibernate { account: id(3) }));
        let fx = tick_range(&mut c, 1_000, 31_000);
        assert!(logs(&fx).is_empty() && !fx.iter().any(|e| matches!(e, Effect::Reload(_))));
        let fx = c.handle(Input::Tick {
            now_ms: 31_000 + WAKE_GAP_MS + 1,
        });
        assert!(logs(&fx)[0].contains("wake detected"));
        let reloads: Vec<_> = fx
            .iter()
            .filter(|e| matches!(e, Effect::Reload(_)))
            .cloned()
            .collect();
        assert_eq!(reloads, vec![Effect::Reload(id(1)), Effect::Reload(id(2))]);
        // exactly 30 s is not a wake
        let fx = c.handle(Input::Tick {
            now_ms: 31_000 + WAKE_GAP_MS + 1 + WAKE_GAP_MS,
        });
        assert!(logs(&fx).is_empty());

        let settings = AppSettings {
            reload_after_wake: false,
            ..AppSettings::default()
        };
        c.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
        let fx = c.handle(Input::Tick { now_ms: 10_000_000 });
        assert!(logs(&fx)[0].contains("wake detected"));
        assert!(!fx.iter().any(|e| matches!(e, Effect::Reload(_))));
    }

    #[test]
    fn disable_and_enable_account() {
        let (mut c, _) = started(vec![acc(1, "discord", "A"), acc(2, "discord", "B")]);
        c.handle(Input::TitleChanged {
            account: id(1),
            title: "(2) A".into(),
        });
        let fx = update_account(&mut c, 1, |s| s.enabled = false);
        assert_eq!(fx[0], Effect::Destroy(id(1)));
        assert!(has(&fx, &Effect::Show(id(2))));
        assert_eq!(badges(&fx), vec![DockBadge::None]);
        let s = snap(&fx);
        assert_eq!(s.active, Some(id(2)));
        assert!(
            s.accounts[0].hibernated
                && !s.accounts[0].settings.enabled
                && s.accounts[0].unread == 0
        );
        assert_eq!(
            menus(&fx)[0]
                .accounts
                .iter()
                .map(|a| a.id)
                .collect::<Vec<_>>(),
            vec![id(2)]
        );
        assert_eq!(polls(&c.handle(Input::Tick { now_ms: 1 })), vec![id(2)]);
        // selecting a disabled account is refused
        let fx = c.handle(Input::Ui(UiCommand::Select { account: id(1) }));
        assert!(
            !logs(&fx).is_empty()
                && !fx
                    .iter()
                    .any(|e| matches!(e, Effect::Show(_) | Effect::CreateView { .. }))
        );
        assert_eq!(c.snapshot().active, Some(id(2)));

        let fx = update_account(&mut c, 1, |s| s.enabled = true);
        assert_eq!(
            views(&fx)
                .iter()
                .map(|(s, v)| (s.account, *v))
                .collect::<Vec<_>>(),
            vec![(id(1), false)]
        );
        assert!(!snap(&fx).accounts[0].hibernated);
        assert_eq!(
            polls(&c.handle(Input::Tick { now_ms: 2 })),
            vec![id(1), id(2)]
        );
    }

    #[test]
    fn disabled_accounts_at_start() {
        let mut a = acc(1, "slack", "A");
        a.enabled = false;
        let mut c = core_with(vec![a, acc(2, "slack", "B")]);
        c.persisted.settings.active = Some(id(1));
        c.active = Some(id(1));
        let fx = c.handle(Input::Started {
            width: 800.0,
            height: 600.0,
        });
        assert_eq!(
            views(&fx)
                .iter()
                .map(|(s, v)| (s.account, *v))
                .collect::<Vec<_>>(),
            vec![(id(2), true)]
        );
        assert_eq!(snap(&fx).active, Some(id(2)));
        assert!(snap(&fx).accounts[0].hibernated);
    }

    #[test]
    fn custom_service_crud() {
        let (mut c, _) = started(vec![]);
        let fx = c.handle(Input::Ui(UiCommand::AddCustomService {
            service: custom(" My Chat ", "https://chat.example.org/app"),
        }));
        let s = snap(&fx);
        assert_eq!(s.custom_services.len(), 1);
        assert_eq!(
            (
                s.custom_services[0].id.as_str(),
                s.custom_services[0].name.as_str()
            ),
            ("custom-my-chat", "My Chat")
        );
        let info = s
            .services
            .iter()
            .find(|x| x.id == "custom-my-chat")
            .unwrap();
        assert!(info.custom && info.name == "My Chat" && info.color == "#123456");
        assert!(!s.services.iter().find(|x| x.id == "slack").unwrap().custom);
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Persist(p) if p.custom_services.len() == 1))
        );
        let fx = c.handle(Input::Ui(UiCommand::AddCustomService {
            service: custom("My chat", "https://b.org"),
        }));
        assert_eq!(snap(&fx).custom_services[1].id, "custom-my-chat-2");

        // invalid input: logged, nothing stored
        for bad in [custom(" ", "https://a.org"), custom("X", "mailto:a@b.c"), {
            let mut x = custom("X", "https://a.org");
            x.title_regex = Some("(".into());
            x
        }] {
            let fx = c.handle(Input::Ui(UiCommand::AddCustomService { service: bad }));
            assert!(
                matches!(fx.as_slice(), [Effect::Log(_), Effect::EmitSnapshot(_)]),
                "{fx:?}"
            );
        }
        assert_eq!(c.snapshot().custom_services.len(), 2);

        // an account of the custom service: title unread with the default regex
        let fx = c.handle(Input::Ui(UiCommand::AddAccount {
            service: "custom-my-chat".into(),
            label: "".into(),
            instance: None,
        }));
        let (spec, _) = views(&fx)[0];
        assert_eq!(spec.url.as_str(), "https://chat.example.org/app");
        assert_eq!(spec.internal_hosts, vec!["chat.example.org"]);
        assert_eq!(spec.user_agent.as_deref(), Some(crate::catalog::UA_SAFARI));
        let fx = c.handle(Input::TitleChanged {
            account: id(101),
            title: "(4) Chat".into(),
        });
        assert_eq!(snap(&fx).accounts[0].unread, 4);
        assert_eq!(snap(&fx).accounts[0].settings.label, "My Chat");

        // update recreates the views of its accounts
        let mut changed = c.snapshot().custom_services[0].clone();
        changed.url = "https://new.example.org/".into();
        let fx = c.handle(Input::Ui(UiCommand::UpdateCustomService {
            service: changed.clone(),
        }));
        assert_eq!(destroys(&fx), vec![id(101)]);
        assert_eq!(views(&fx)[0].0.url.as_str(), "https://new.example.org/");
        assert!(views(&fx)[0].1);
        changed.url = "nope".into();
        let fx = c.handle(Input::Ui(UiCommand::UpdateCustomService {
            service: changed.clone(),
        }));
        assert!(matches!(
            fx.as_slice(),
            [Effect::Log(_), Effect::EmitSnapshot(_)]
        ));
        changed.id = "custom-unknown".into();
        let fx = c.handle(Input::Ui(UiCommand::UpdateCustomService {
            service: changed,
        }));
        assert!(matches!(
            fx.as_slice(),
            [Effect::Log(_), Effect::EmitSnapshot(_)]
        ));

        // remove: refused while used
        let fx = c.handle(Input::Ui(UiCommand::RemoveCustomService {
            id: "custom-my-chat".into(),
        }));
        assert!(logs(&fx)[0].contains("still used"));
        assert_eq!(snap(&fx).custom_services.len(), 2);
        c.handle(Input::Ui(UiCommand::RemoveAccount { account: id(101) }));
        let fx = c.handle(Input::Ui(UiCommand::RemoveCustomService {
            id: "custom-my-chat".into(),
        }));
        let s = snap(&fx);
        assert_eq!(
            s.custom_services
                .iter()
                .map(|x| x.id.as_str())
                .collect::<Vec<_>>(),
            vec!["custom-my-chat-2"]
        );
        assert!(s.services.iter().all(|x| x.id != "custom-my-chat"));
    }

    #[test]
    fn custom_services_loaded_from_persisted() {
        let mut bad = custom("Bad", "nope");
        bad.id = "custom-bad".into();
        let mut good = custom("Good", "https://{instance}.good.io/");
        good.id = "custom-good".into();
        let mut a = acc(1, "custom-good", "G");
        a.instance = Some("kit".into());
        let persisted = Persisted {
            accounts: vec![a],
            custom_services: vec![bad, good],
            ..Default::default()
        };
        let mut c = Core::new(Catalog::builtin(), persisted, Box::new(|| id(5)));
        let fx = c.handle(Input::Started {
            width: 800.0,
            height: 600.0,
        });
        assert!(logs(&fx).iter().any(|l| l.contains("Bad")));
        assert_eq!(views(&fx)[0].0.url.as_str(), "https://kit.good.io/");
        assert_eq!(
            snap(&fx).accounts[0].instance_label.as_deref(),
            Some("Instance")
        );
    }

    #[test]
    fn favicon_flow() {
        let (mut c, _) = started(vec![acc(1, "slack", "A")]);
        let loaded = |c: &mut Core| {
            c.handle(Input::PageLoaded {
                account: id(1),
                url: "https://app.slack.com".into(),
            })
        };
        let fx = loaded(&mut c);
        assert_eq!(
            fx,
            vec![Effect::FindIcon {
                account: id(1),
                script: ICON_SCRIPT.into()
            }]
        );
        // retried on later page loads (single-page apps set their icon late), at most 3 times
        assert_eq!(loaded(&mut c).len(), 1);
        assert_eq!(loaded(&mut c).len(), 1);
        assert!(
            loaded(&mut c).is_empty(),
            "at most MAX_ICON_ATTEMPTS per run"
        );
        let raw = serde_json::to_string("https://a.slack-edge.com/icon.png").unwrap();
        let fx = c.handle(Input::IconFound {
            account: id(1),
            raw,
        });
        assert_eq!(
            fx,
            vec![Effect::FetchIcon {
                account: id(1),
                url: "https://a.slack-edge.com/icon.png".into()
            }]
        );
        let fx = c.handle(Input::IconFetched {
            account: id(1),
            data_url: None,
        });
        assert!(matches!(fx.as_slice(), [Effect::Log(_)]));
        let data = "data:image/png;base64,AAAA".to_string();
        let fx = c.handle(Input::IconFetched {
            account: id(1),
            data_url: Some(data.clone()),
        });
        assert!(fx.iter().any(
            |e| matches!(e, Effect::Persist(p) if p.accounts[0].favicon.as_deref() == Some(&*data))
        ));
        assert_eq!(snap(&fx).accounts[0].icon.as_deref(), Some(&*data));
        // a custom icon wins
        let fx = update_account(&mut c, 1, |s| {
            s.custom_icon = Some(" https://x.org/me.png ".into())
        });
        assert_eq!(
            snap(&fx).accounts[0].icon.as_deref(),
            Some("https://x.org/me.png")
        );
        // refresh looks it up again, even with a cached favicon
        let fx = c.handle(Input::Ui(UiCommand::RefreshIcon { account: id(1) }));
        assert_eq!(
            fx,
            vec![Effect::FindIcon {
                account: id(1),
                script: ICON_SCRIPT.into()
            }]
        );
        // data URLs are taken directly; bad results are logged
        let raw = serde_json::to_string("data:image/svg+xml;base64,BB").unwrap();
        let fx = c.handle(Input::IconFound {
            account: id(1),
            raw,
        });
        assert!(fx.iter().any(|e| matches!(e, Effect::Persist(p) if p.accounts[0].favicon.as_deref() == Some("data:image/svg+xml;base64,BB"))));
        for raw in ["null", "\"javascript:alert(1)\"", "garbage"] {
            let fx = c.handle(Input::IconFound {
                account: id(1),
                raw: raw.into(),
            });
            assert!(matches!(fx.as_slice(), [Effect::Log(_)]), "{raw}: {fx:?}");
        }
        // a cached favicon is not looked up again on the next run
        let mut c2 = core_with(c.persisted.accounts.clone());
        c2.handle(Input::Started {
            width: 800.0,
            height: 600.0,
        });
        assert!(
            c2.handle(Input::PageLoaded {
                account: id(1),
                url: "x".into()
            })
            .is_empty()
        );
    }

    fn svg(tag: &str) -> String {
        format!("data:image/svg+xml;base64,{tag}")
    }

    fn fetched(c: &mut Core, n: u128, slug: &str, r: SimpleIconResult) -> Vec<Effect> {
        c.handle(Input::SimpleIconFetched {
            account: id(n),
            slug: slug.into(),
            result: r,
        })
    }

    #[test]
    fn slug_normalisation() {
        assert_eq!(normalize_slug("  Discord "), Ok(Some("discord".into())));
        assert_eq!(normalize_slug("  "), Ok(None));
        assert_eq!(normalize_slug("x2"), Ok(Some("x2".into())));
        for bad in ["a b", "dis.cord", "../x", "é", "a/b", "a-b"] {
            assert!(normalize_slug(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn simple_icon_fetch_on_set_and_change() {
        let (mut c, _) = started(vec![acc(1, "discord", "D")]);
        let fx = update_account(&mut c, 1, |s| s.simple_icon = Some(" Discord ".into()));
        assert!(fx.contains(&Effect::FetchSimpleIcon {
            account: id(1),
            slug: "discord".into()
        }));
        assert_eq!(
            c.account(id(1)).unwrap().simple_icon.as_deref(),
            Some("discord")
        );
        // unchanged slug: no refetch
        let fx = update_account(&mut c, 1, |s| s.muted = true);
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::FetchSimpleIcon { .. }))
        );
        let fx = fetched(&mut c, 1, "discord", SimpleIconResult::Found(svg("AA")));
        assert!(fx.iter().any(|e| matches!(e, Effect::Persist(p) if p.accounts[0].simple_icon_data.as_deref() == Some(&*svg("AA")))));
        // change: cached glyph dropped, new fetch; a stale result is ignored
        let fx = update_account(&mut c, 1, |s| s.simple_icon = Some("matrix".into()));
        assert!(fx.contains(&Effect::FetchSimpleIcon {
            account: id(1),
            slug: "matrix".into()
        }));
        assert!(c.account(id(1)).unwrap().simple_icon_data.is_none());
        assert!(fetched(&mut c, 1, "discord", SimpleIconResult::Found(svg("BB"))).is_empty());
        assert!(c.account(id(1)).unwrap().simple_icon_data.is_none());
        // clearing the slug fetches nothing
        let fx = update_account(&mut c, 1, |s| s.simple_icon = None);
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::FetchSimpleIcon { .. }))
        );
        assert!(c.account(id(1)).unwrap().simple_icon.is_none());
    }

    #[test]
    fn simple_icon_invalid_slug_logged_no_glyph() {
        let (mut c, _) = started(vec![acc(1, "discord", "D")]);
        let fx = update_account(&mut c, 1, |s| s.simple_icon = Some("dis cord".into()));
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Log(m) if m.contains("invalid Simple Icons slug")))
        );
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::FetchSimpleIcon { .. }))
        );
        assert!(c.account(id(1)).unwrap().simple_icon.is_none());
    }

    #[test]
    fn simple_icon_fetched_on_start_when_missing() {
        let mut a = acc(1, "discord", "D");
        a.simple_icon = Some("discord".into());
        let mut b = acc(2, "discord", "E");
        b.simple_icon = Some("matrix".into());
        b.simple_icon_data = Some(svg("CC"));
        let (_, fx) = started(vec![a, b]);
        let n = fx
            .iter()
            .filter(|e| matches!(e, Effect::FetchSimpleIcon { .. }))
            .count();
        assert_eq!(n, 1);
        assert!(fx.contains(&Effect::FetchSimpleIcon {
            account: id(1),
            slug: "discord".into()
        }));
    }

    #[test]
    fn simple_icon_precedence_and_404_fallback() {
        let (mut c, _) = started(vec![acc(1, "slack", "S")]);
        let fav = "data:image/png;base64,FF".to_string();
        c.handle(Input::IconFetched {
            account: id(1),
            data_url: Some(fav.clone()),
        });
        update_account(&mut c, 1, |s| s.simple_icon = Some("slack".into()));
        // 404: logged, favicon stays
        let fx = fetched(&mut c, 1, "slack", SimpleIconResult::NotFound);
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Log(m) if m == "Simple Icons has no icon 'slack'"))
        );
        let s = c.snapshot();
        assert_eq!(
            (s.accounts[0].icon.as_deref(), s.accounts[0].icon_glyph),
            (Some(&*fav), false)
        );
        // glyph beats favicon
        let fx = fetched(&mut c, 1, "slack", SimpleIconResult::Found(svg("GG")));
        let v = &snap(&fx).accounts[0];
        assert_eq!((v.icon.as_deref(), v.icon_glyph), (Some(&*svg("GG")), true));
        // custom icon beats glyph
        let fx = update_account(&mut c, 1, |s| {
            s.custom_icon = Some("https://x.org/me.png".into())
        });
        let v = &snap(&fx).accounts[0];
        assert_eq!(
            (v.icon.as_deref(), v.icon_glyph),
            (Some("https://x.org/me.png"), false)
        );
        // slug without cached glyph and without favicon: initials
        let (c2, _) = started(vec![{
            let mut a = acc(1, "slack", "S");
            a.simple_icon = Some("slack".into());
            a
        }]);
        let v = &c2.snapshot().accounts[0];
        assert_eq!((v.icon.clone(), v.icon_glyph), (None, false));
    }

    #[test]
    fn zoom_menu_and_settings() {
        let (mut c, _) = started(vec![acc(1, "slack", "A")]);
        let zoom = |fx: &[Effect]| {
            fx.iter().find_map(|e| {
                if let Effect::SetZoom { zoom, .. } = e {
                    Some(*zoom)
                } else {
                    None
                }
            })
        };
        assert_eq!(zoom(&c.handle(Input::Menu(MenuAction::ZoomIn))), Some(1.1));
        assert_eq!(zoom(&c.handle(Input::Menu(MenuAction::ZoomOut))), Some(1.0));
        let fx = c.handle(Input::Menu(MenuAction::ZoomOut));
        assert_eq!(zoom(&fx), Some(0.9));
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Persist(p) if p.accounts[0].zoom == 0.9))
        );
        assert_eq!(
            zoom(&c.handle(Input::Menu(MenuAction::ZoomReset))),
            Some(1.0)
        );
        assert!(c.handle(Input::Menu(MenuAction::ZoomReset)).is_empty());
        let fx = update_account(&mut c, 1, |s| s.zoom = 1.3);
        assert_eq!(zoom(&fx), Some(1.3));
        assert!(!fx.iter().any(|e| matches!(e, Effect::CreateView { .. })));
        assert_eq!(zoom(&c.handle(Input::Menu(MenuAction::ZoomIn))), Some(1.5));
        let fx = update_account(&mut c, 1, |s| s.zoom = 2.0);
        assert_eq!(zoom(&fx), Some(2.0));
        assert!(c.handle(Input::Menu(MenuAction::ZoomIn)).is_empty());
        assert_eq!(
            zoom(&update_account(&mut c, 1, |s| s.zoom = f64::NAN)),
            Some(1.0)
        );
        // a new view gets the persisted zoom
        update_account(&mut c, 1, |s| s.zoom = 0.8);
        let fx = update_account(&mut c, 1, |s| s.user_agent = Some("chrome".into()));
        assert_eq!(zoom(&fx), Some(0.8));
    }

    #[test]
    fn menu_actions_on_active_account() {
        let (mut c, _) = started(vec![acc(1, "slack", "A"), acc(2, "slack", "B")]);
        c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
        assert_eq!(
            c.handle(Input::Menu(MenuAction::Reload)),
            vec![Effect::Reload(id(2))]
        );
        assert_eq!(
            c.handle(Input::Menu(MenuAction::Back)),
            vec![Effect::Eval {
                account: id(2),
                script: "history.back()".into()
            }]
        );
        assert_eq!(
            c.handle(Input::Ui(UiCommand::GoForward { account: id(1) })),
            vec![Effect::Eval {
                account: id(1),
                script: "history.forward()".into()
            }]
        );
        assert_eq!(
            c.handle(Input::Menu(MenuAction::OpenInBrowser)),
            vec![Effect::OpenInBrowser {
                account: id(2),
                browser: None
            }]
        );
        let fx = c.handle(Input::Menu(MenuAction::ToggleDnd));
        assert!(snap(&fx).settings.dnd);
        assert!(menus(&fx)[0].dnd);
        let fx = c.handle(Input::Menu(MenuAction::ToggleDnd));
        assert!(!snap(&fx).settings.dnd);
        let (mut empty, _) = started(vec![]);
        assert!(matches!(
            empty.handle(Input::Menu(MenuAction::Reload)).as_slice(),
            [Effect::Log(_)]
        ));
    }

    #[test]
    fn open_in_browser_and_clear_data() {
        let (mut c, _) = started(vec![acc(1, "slack", "A"), acc(2, "slack", "B")]);
        let fx = c.handle(Input::Ui(UiCommand::ClearData { account: id(2) }));
        assert_eq!(fx.last(), Some(&Effect::ClearData(id(2))));
        c.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
        let fx = c.handle(Input::Ui(UiCommand::ClearData { account: id(2) }));
        assert_eq!(
            fx.last(),
            Some(&Effect::DeleteProfile {
                store_id: *id(2).as_bytes()
            })
        );
        let fx = c.handle(Input::Ui(UiCommand::OpenInBrowser { account: id(2) }));
        assert_eq!(
            fx,
            vec![Effect::OpenExternal {
                url: "https://app.slack.com/client".into(),
                browser: None
            }]
        );
    }

    #[test]
    fn menus_follow_accounts_unread_and_settings() {
        let (mut c, fx) = started(vec![acc(1, "discord", "A"), acc(2, "discord", "B")]);
        let m = menus(&fx)[0].clone();
        assert_eq!(
            m.accounts
                .iter()
                .map(|a| a.label.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B"]
        );
        assert!(m.show_tray && !m.dnd && m.tray_title.is_empty());
        assert!(
            menus(&c.handle(Input::Tick { now_ms: 1 })).is_empty(),
            "unchanged -> not re-emitted"
        );
        let fx = c.handle(Input::TitleChanged {
            account: id(2),
            title: "(3) B".into(),
        });
        let m = menus(&fx)[0];
        assert_eq!((m.accounts[1].unread, m.tray_title.as_str()), (3, "3"));
        let fx = c.handle(Input::TitleChanged {
            account: id(2),
            title: "• B".into(),
        });
        assert_eq!(menus(&fx)[0].tray_title, "•");
        let settings = AppSettings {
            show_tray: false,
            ..AppSettings::default()
        };
        let fx = c.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
        assert!(!menus(&fx)[0].show_tray);
        let fx = update_account(&mut c, 1, |s| s.label = "Work".into());
        assert_eq!(menus(&fx)[0].accounts[0].label, "Work");
        assert!(
            menus(&core_with(vec![]).handle(Input::Tick { now_ms: 1 })).is_empty(),
            "nothing before start"
        );
    }

    #[test]
    fn custom_css_injected_and_recreates() {
        let (mut c, _) = started(vec![acc(1, "slack", "A")]);
        let fx = update_account(&mut c, 1, |s| {
            s.custom_css = Some("body{content:\"</style>\\n\"}".into())
        });
        assert_eq!(destroys(&fx), vec![id(1)]);
        let script = &views(&fx)[0].0.init_script;
        assert!(script.starts_with(crate::NOTIFY_SHIM));
        assert!(
            script.contains(r#"const css="body{content:\"</style>\\n\"}";"#),
            "{script}"
        );
        let fx = update_account(&mut c, 1, |s| s.label = "B".into());
        assert!(destroys(&fx).is_empty(), "same CSS -> no recreate");
        let fx = update_account(&mut c, 1, |s| s.custom_css = Some("  ".into()));
        assert_eq!(views(&fx)[0].0.init_script, crate::NOTIFY_SHIM);
        assert_eq!(c.account(id(1)).unwrap().custom_css, None);
    }

    #[test]
    fn settings_of_hibernated_account_apply_on_next_load() {
        let (mut c, _) = started(vec![acc(1, "slack", "A"), acc(2, "slack", "B")]);
        c.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
        let fx = update_account(&mut c, 2, |s| {
            s.user_agent = Some("chrome".into());
            s.zoom = 1.5;
            s.hibernate_after_min = Some(0);
        });
        assert!(!fx.iter().any(|e| matches!(
            e,
            Effect::CreateView { .. } | Effect::SetZoom { .. } | Effect::Destroy(_)
        )));
        assert_eq!(c.account(id(2)).unwrap().hibernate_after_min, Some(0));
        let fx = c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
        assert_eq!(
            views(&fx)[0].0.user_agent.as_deref(),
            Some(crate::catalog::UA_CHROME)
        );
        assert!(has(
            &fx,
            &Effect::SetZoom {
                account: id(2),
                zoom: 1.5
            }
        ));
    }
    // ---- expert mode: user service files ----

    fn user_def(id: &str, url: &str) -> ServiceDef {
        Catalog::parse(
            "t.toml",
            &format!("id = \"{id}\"\nname = \"X {id}\"\ncolor = \"#111111\"\nurl = \"{url}\"\n"),
        )
        .unwrap()
    }

    fn created(fx: &[Effect]) -> Vec<Uuid> {
        fx.iter()
            .filter_map(|e| match e {
                Effect::CreateView { spec, .. } => Some(spec.account),
                _ => None,
            })
            .collect()
    }

    fn two_user_accounts() -> Core {
        let mut c = core_with(vec![acc(1, "alpha", "A"), acc(2, "beta", "B")]);
        c.set_user_services(
            vec![
                user_def("alpha", "https://a.example/"),
                user_def("beta", "https://b.example/"),
            ],
            vec![],
        );
        let fx = c.handle(Input::Started {
            width: 1000.0,
            height: 600.0,
        });
        assert_eq!(created(&fx).len(), 2);
        c
    }

    #[test]
    fn open_and_reload_commands_emit_effects() {
        let (mut c, _) = started(vec![]);
        assert_eq!(
            c.handle(Input::Ui(UiCommand::OpenServicesFolder))[0],
            Effect::OpenServicesFolder
        );
        assert_eq!(
            c.handle(Input::Ui(UiCommand::ReloadServices))[0],
            Effect::LoadServices
        );
    }

    #[test]
    fn reload_replaces_definition_and_recreates_only_affected_account() {
        let mut c = two_user_accounts();
        let fx = c.handle(Input::ServicesLoaded {
            defs: vec![
                user_def("alpha", "https://a2.example/"),
                user_def("beta", "https://b.example/"),
            ],
            errors: vec![],
        });
        assert_eq!(created(&fx), vec![id(1)]);
        assert!(fx.contains(&Effect::Destroy(id(1))));
        assert!(!fx.contains(&Effect::Destroy(id(2))));
        // the unaffected account only gets its policy again
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::SetViewPolicy { account, .. } if *account == id(2)))
        );
        let s0 = snap(&fx);
        let alpha = s0.services.iter().find(|s| s.id == "alpha").unwrap();
        assert_eq!(alpha.url, "https://a2.example/");
        assert!(s0.service_errors.is_empty());
    }

    #[test]
    fn reload_error_is_reported_and_keeps_previous_definition() {
        let mut c = two_user_accounts();
        let fx = c.handle(Input::ServicesLoaded {
            defs: vec![user_def("beta", "https://b.example/")],
            errors: vec!["invalid service definition: broken.toml".into()],
        });
        assert!(created(&fx).is_empty());
        assert!(fx.contains(&Effect::Log(
            "invalid service definition: broken.toml".into()
        )));
        let s0 = snap(&fx);
        assert_eq!(s0.service_errors.len(), 1);
        let alpha = s0.services.iter().find(|s| s.id == "alpha").unwrap();
        assert_eq!(alpha.url, "https://a.example/");
        // without errors, a missing file removes its definition
        let fx = c.handle(Input::ServicesLoaded {
            defs: vec![user_def("beta", "https://b.example/")],
            errors: vec![],
        });
        assert!(snap(&fx).services.iter().all(|s| s.id != "alpha"));
        assert!(snap(&fx).service_errors.is_empty());
    }

    #[test]
    fn user_file_overrides_builtin_and_custom_services_win() {
        let (mut c, _) = started(vec![]);
        let fx = c.handle(Input::ServicesLoaded {
            defs: vec![user_def("slack", "https://my.slack.example/")],
            errors: vec![],
        });
        let slack = snap(&fx)
            .services
            .into_iter()
            .find(|s| s.id == "slack")
            .unwrap();
        assert_eq!(slack.url, "https://my.slack.example/");
        assert_eq!(slack.name, "X slack");
        assert_eq!(
            snap(&fx)
                .services
                .iter()
                .filter(|s| s.id == "slack")
                .count(),
            1
        );
    }

    #[test]
    fn startup_user_services_errors_reach_snapshot() {
        let mut c = core_with(vec![]);
        c.set_user_services(vec![], vec!["bad file".into()]);
        assert_eq!(c.snapshot().service_errors, vec!["bad file".to_string()]);
    }
}
