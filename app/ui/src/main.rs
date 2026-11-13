mod tauri;

use leptos::prelude::*;
use protocol::{
    AccountSettings, AccountView, AppInfo, AppSettings, CustomService, DockBadgeMode, IconSize,
    LinkTarget, ServiceInfo, Snapshot, UiCommand,
};
use std::time::Duration;
use uuid::Uuid;
use wasm_bindgen::{JsCast, prelude::Closure};

fn send(cmd: UiCommand) {
    leptos::task::spawn_local(tauri::send(cmd));
}

/// Inline white glyph standing in for a Simple Icons download in the demo.
const DEMO_GLYPH: &str = "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path fill='white' d='M12 2 2 7v10l10 5 10-5V7zm0 4a6 6 0 1 1 0 12 6 6 0 0 1 0-12z'/></svg>";
const DEMO_ICON: &str = "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'><rect width='32' height='32' rx='8' fill='%23e11d48'/><circle cx='16' cy='16' r='8' fill='white'/></svg>";

fn demo_snapshot() -> Snapshot {
    let acc = |n: u128,
               service: &str,
               name: &str,
               label: &str,
               color: &str,
               ini: &str,
               unread,
               indirect,
               muted,
               team_label: Option<&str>| {
        AccountView {
            id: Uuid::from_u128(n),
            service: service.into(),
            service_name: name.into(),
            color: color.into(),
            initials: ini.into(),
            unread,
            indirect,
            team_label: team_label.map(Into::into),
            icon: None,
            icon_glyph: false,
            hibernated: false,
            settings: AccountSettings {
                label: label.into(),
                team: team_label.map(|_| "example".to_string()),
                muted,
                show_badge: true,
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
            },
        }
    };
    let mut accounts = vec![
        acc(
            1,
            "slack",
            "Slack",
            "KIT",
            "#4A154B",
            "S",
            3,
            false,
            false,
            Some("Workspace"),
        ),
        acc(
            2,
            "element",
            "Element",
            "Home",
            "#0DBD8B",
            "E",
            0,
            true,
            false,
            Some("Instance"),
        ),
        acc(
            3, "discord", "Discord", "Friends", "#5865F2", "D", 120, false, true, None,
        ),
        acc(
            4,
            "slack",
            "Slack",
            "Work",
            "#4A154B",
            "SW",
            0,
            false,
            false,
            Some("Workspace"),
        ),
        acc(
            5,
            "custom-wiki",
            "Wiki",
            "Team wiki",
            "#0F766E",
            "W",
            0,
            false,
            false,
            None,
        ),
    ];
    accounts[0].icon = Some(DEMO_ICON.into());
    accounts[0].settings.custom_icon = Some(DEMO_ICON.into());
    accounts[2].icon = Some(DEMO_GLYPH.into());
    accounts[2].icon_glyph = true;
    accounts[2].settings.simple_icon = Some("discord".into());
    accounts[1].hibernated = true;
    accounts[1].settings.hibernate_after_min = Some(15);
    accounts[3].settings.enabled = false;
    accounts[3].hibernated = true;
    let custom = CustomService {
        id: "custom-wiki".into(),
        name: "Wiki".into(),
        url: "https://{team}/".into(),
        color: "#0f766e".into(),
        team_label: Some("Host".into()),
        title_regex: None,
        user_agent: None,
    };
    Snapshot {
        accounts,
        active: Some(Uuid::from_u128(1)),
        services: vec![
            ServiceInfo {
                id: "slack".into(),
                name: "Slack".into(),
                color: "#4A154B".into(),
                team_label: Some("Workspace".into()),
                team_default: None,
                custom: false,
                allow_media: false,
                url: "https://{team}.slack.com/".into(),
                title_regex: Some(r"^\((\d+)\)".into()),
                user_agent: None,
            },
            ServiceInfo {
                id: "element".into(),
                name: "Element".into(),
                color: "#0DBD8B".into(),
                team_label: Some("Instance".into()),
                team_default: Some("app.element.io".into()),
                custom: false,
                allow_media: false,
                url: "https://{team}/".into(),
                title_regex: Some(r"^\[(\d+)\]".into()),
                user_agent: Some("chrome".into()),
            },
            ServiceInfo {
                id: "mastodon".into(),
                name: "Mastodon".into(),
                color: "#6364FF".into(),
                team_label: Some("Instance".into()),
                team_default: Some("mastodon.social".into()),
                custom: false,
                allow_media: false,
                url: "https://{team}/".into(),
                title_regex: None,
                user_agent: Some("Mozilla/5.0 (Macintosh) Mastodon".into()),
            },
            ServiceInfo {
                id: "discord".into(),
                name: "Discord".into(),
                color: "#5865F2".into(),
                team_label: None,
                team_default: None,
                custom: false,
                allow_media: false,
                url: "https://discord.com/app".into(),
                title_regex: None,
                user_agent: None,
            },
            ServiceInfo {
                id: "custom-wiki".into(),
                name: "Wiki".into(),
                color: "#0f766e".into(),
                team_label: Some("Host".into()),
                team_default: None,
                custom: true,
                allow_media: false,
                url: "https://{team}/".into(),
                title_regex: None,
                user_agent: None,
            },
        ],
        custom_services: vec![custom],
        dialog_open: false,
        settings: AppSettings::default(),
        service_errors: vec![],
        app: AppInfo {
            version: "0.1.0-demo".into(),
            services_dir: "~/Library/Application Support/messengers/services".into(),
            config_dir: "~/Library/Application Support/messengers".into(),
            log_file: "~/Library/Logs/messengers/messengers.log".into(),
        },
    }
}

fn query_params() -> Vec<(String, String)> {
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    search
        .trim_start_matches('?')
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (p.to_string(), String::new()),
        })
        .collect()
}

fn query_param(key: &str) -> Option<String> {
    query_params()
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// `?demo` wins over a present `window.__TAURI__`.
fn is_demo() -> bool {
    query_param("demo").is_some()
}

/// Talk to the Rust side only when not in demo mode.
fn use_tauri() -> bool {
    !is_demo() && tauri::available()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Accounts,
    Custom,
    General,
    Notifications,
    Dock,
    Links,
    About,
}

impl Tab {
    const ALL: [Tab; 7] = [
        Tab::Accounts,
        Tab::Custom,
        Tab::General,
        Tab::Notifications,
        Tab::Dock,
        Tab::Links,
        Tab::About,
    ];
    fn from_id(id: &str) -> Option<Tab> {
        Some(match id {
            "accounts" => Tab::Accounts,
            "general" => Tab::General,
            "notifications" => Tab::Notifications,
            "dock" => Tab::Dock,
            "links" => Tab::Links,
            "custom" => Tab::Custom,
            "about" => Tab::About,
            _ => return None,
        })
    }
    fn title(self) -> &'static str {
        match self {
            Tab::Accounts => "Accounts",
            Tab::Custom => "Custom services",
            Tab::General => "General",
            Tab::Notifications => "Notifications",
            Tab::Dock => "Dock badge",
            Tab::Links => "Links",
            Tab::About => "About",
        }
    }
}

/// Purely local UI state: selected tab and a one-shot request to focus the add form.
#[derive(Clone, Copy)]
struct UiState {
    tab: RwSignal<Tab>,
    focus_add: RwSignal<bool>,
    /// Demo aid: index of the account whose edit form opens initially (consumed once).
    edit_idx: RwSignal<Option<usize>>,
    /// Demo aid: open the custom-service add form initially (consumed once).
    add_custom: RwSignal<bool>,
}

#[derive(Clone, Copy)]
struct Dispatch(WriteSignal<Snapshot>);

#[component]
fn App() -> impl IntoView {
    let (snap, set_snap) = signal(Snapshot::default());
    let mut tab = Tab::Accounts;
    let mut edit_idx = None;
    let mut add_custom = false;
    if use_tauri() {
        tauri::listen_snapshots(move |s| set_snap.set(s));
        leptos::task::spawn_local(async move {
            if let Some(s) = tauri::get_snapshot().await {
                set_snap.set(s);
            }
        });
    } else if is_demo() {
        let mut s = demo_snapshot();
        if let Some(d) = query_param("dialog") {
            if let Some(t) = Tab::from_id(&d) {
                tab = t;
            }
            s.dialog_open = true;
        }
        edit_idx = query_param("edit").and_then(|v| v.parse().ok());
        add_custom = query_param("addcustom").is_some_and(|v| v != "0");
        set_snap.set(s);
    }

    provide_context(Dispatch(set_snap));
    provide_context(UiState {
        tab: RwSignal::new(tab),
        focus_add: RwSignal::new(false),
        edit_idx: RwSignal::new(edit_idx),
        add_custom: RwSignal::new(add_custom),
    });

    let on_key = window_event_listener(leptos::ev::keydown, move |e| {
        if e.key() == "Escape" && snap.with_untracked(|s| s.dialog_open) {
            cmd(UiCommand::SetDialog { open: false });
        }
    });
    on_cleanup(move || on_key.remove());

    // Only notifies when the dialog flag actually flips, not on every snapshot.
    let dialog_open = Memo::new(move |_| snap.with(|s| s.dialog_open));
    let vars = Memo::new(move |_| {
        snap.with(|s| {
            let i = s.settings.icon_size;
            format!(
                "--icon: {}px; --sidebar: {}px; --gap: {}px;",
                i.icon_px(),
                i.sidebar_width(),
                (i.icon_px() * 0.25).round()
            )
        })
    });
    view! {
        <div class="root" style=move || vars.get()>
            <Sidebar snap=snap />
            <Show when=move || dialog_open.get()>
                <SettingsDialog snap=snap />
            </Show>
        </div>
    }
}

/// Send to Rust, or apply locally when running as a browser demo.
fn dispatch(set: WriteSignal<Snapshot>, cmd: UiCommand) {
    if use_tauri() {
        send(cmd);
        return;
    }
    set.update(|s| match cmd {
        UiCommand::Select { account } => s.active = Some(account),
        UiCommand::SetDialog { open } => s.dialog_open = open,
        UiCommand::SetDnd { enabled } => s.settings.dnd = enabled,
        UiCommand::UpdateSettings { settings } => s.settings = settings,
        UiCommand::UpdateAccount { account, settings } => {
            if let Some(a) = s.accounts.iter_mut().find(|a| a.id == account) {
                a.settings = settings;
            }
        }
        UiCommand::SetMuted { account, muted } => {
            if let Some(a) = s.accounts.iter_mut().find(|a| a.id == account) {
                a.settings.muted = muted;
            }
        }
        UiCommand::RemoveAccount { account } => s.accounts.retain(|a| a.id != account),
        UiCommand::Hibernate { account } => {
            if let Some(a) = s.accounts.iter_mut().find(|a| a.id == account) {
                a.hibernated = true;
            }
        }
        UiCommand::AddCustomService { mut service } => {
            service.id = format!("custom-{}", service.name.to_lowercase().replace(' ', "-"));
            s.services.push(ServiceInfo {
                id: service.id.clone(),
                name: service.name.clone(),
                color: service.color.clone(),
                team_label: service.team_label.clone(),
                team_default: None,
                custom: true,
                allow_media: false,
                url: service.url.clone(),
                title_regex: service.title_regex.clone(),
                user_agent: service.user_agent.clone(),
            });
            s.custom_services.push(service);
        }
        UiCommand::UpdateCustomService { service } => {
            if let Some(c) = s.custom_services.iter_mut().find(|c| c.id == service.id) {
                *c = service;
            }
        }
        UiCommand::RemoveCustomService { id } => {
            if !s.accounts.iter().any(|a| a.service == id) {
                s.custom_services.retain(|c| c.id != id);
                s.services.retain(|c| c.id != id);
            }
        }
        UiCommand::Move { account, delta } => {
            if let Some(i) = s.accounts.iter().position(|a| a.id == account) {
                let j = i as i64 + delta as i64;
                let j = j.clamp(0, s.accounts.len() as i64 - 1) as usize;
                let a = s.accounts.remove(i);
                s.accounts.insert(j, a);
            }
        }
        _ => {}
    });
}

fn cmd(c: UiCommand) {
    if let Some(Dispatch(set)) = use_context::<Dispatch>() {
        dispatch(set, c);
    }
}

fn badge_text(n: u32) -> String {
    if n > 99 { "99+".into() } else { n.to_string() }
}

fn open_dialog(tab: Tab, focus_add: bool) {
    if let Some(ui) = use_context::<UiState>() {
        ui.tab.set(tab);
        ui.focus_add.set(focus_add);
    }
    cmd(UiCommand::SetDialog { open: true });
}

/// Pointer-drag bookkeeping, measured once at pointerdown so snapshots cannot disturb it.
#[derive(Clone)]
struct Pending {
    id: Uuid,
    pointer: i32,
    y0: f64,
    source: usize,
    centers: Vec<f64>,
    active: bool,
    dead: bool,
}

/// Visible drag state that drives the per-icon transforms.
#[derive(Clone, Copy, PartialEq)]
struct Drag {
    id: Uuid,
    source: usize,
    target: usize,
    dy: f64,
    pitch: f64,
}

fn drag_style(d: Option<Drag>, id: Uuid, idx: usize) -> String {
    let Some(d) = d else { return String::new() };
    if d.id == id {
        return format!(
            "transform: translateY({}px) scale(1.08); transition: none; z-index: 10;",
            d.dy
        );
    }
    let shift = if d.source < d.target && idx > d.source && idx <= d.target {
        -d.pitch
    } else if d.source > d.target && idx >= d.target && idx < d.source {
        d.pitch
    } else {
        0.0
    };
    format!("transform: translateY({shift}px);")
}

#[component]
fn Sidebar(snap: ReadSignal<Snapshot>) -> impl IntoView {
    let dnd = Memo::new(move |_| snap.with(|s| s.settings.dnd));
    let drag: RwSignal<Option<Drag>> = RwSignal::new(None);
    let pending: StoredValue<Option<Pending>> = StoredValue::new(None);
    let suppress = StoredValue::new(false);
    // Rendered order; frozen while a drag is active, resynced from the snapshot afterwards.
    let order: RwSignal<Vec<Uuid>> = RwSignal::new(Vec::new());
    let snap_ids = move || snap.with(|s| s.accounts.iter().map(|a| a.id).collect::<Vec<_>>());
    Effect::new(move |_| {
        let ids = snap_ids();
        if drag.get_untracked().is_none() && order.with_untracked(|o| *o != ids) {
            order.set(ids);
        }
    });

    let finish = move |send_move: bool| {
        let d = drag.get_untracked();
        drag.set(None);
        let mut next = snap_ids();
        if let Some(d) = d {
            if send_move && d.target != d.source {
                // Optimistic local reorder; the next snapshot carries the same order.
                next = order.get_untracked();
                if let Some(i) = next.iter().position(|x| *x == d.id) {
                    let x = next.remove(i);
                    next.insert(d.target.min(next.len()), x);
                }
            }
            suppress.set_value(true);
            // Backstop in case no click follows.
            set_timeout(move || suppress.set_value(false), Duration::from_millis(50));
            if send_move && d.target != d.source {
                cmd(UiCommand::Move {
                    account: d.id,
                    delta: d.target as i32 - d.source as i32,
                });
            }
        }
        order.set(next);
    };
    let list_ref = NodeRef::<leptos::html::Div>::new();
    let on_down = move |e: web_sys::PointerEvent| {
        if e.button() != 0 || !e.is_primary() {
            return;
        }
        e.prevent_default();
        let Some(container) = list_ref
            .get_untracked()
            .map(|d| d.unchecked_into::<web_sys::Element>())
        else {
            return;
        };
        let Some(hit) = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|t| t.closest(".acct").ok().flatten())
        else {
            return;
        };
        let kids = container.children();
        let mut centers = Vec::new();
        let mut source = None;
        for i in 0..kids.length() {
            let Some(k) = kids.item(i) else { continue };
            let r = k.get_bounding_client_rect();
            if k == hit {
                source = Some(centers.len());
            }
            centers.push(r.top() + r.height() / 2.0);
        }
        let (Some(source), Some(id)) = (
            source,
            hit.get_attribute("data-id")
                .and_then(|v| v.parse::<Uuid>().ok()),
        ) else {
            return;
        };
        pending.set_value(Some(Pending {
            id,
            pointer: e.pointer_id(),
            y0: e.client_y() as f64,
            source,
            centers,
            active: false,
            dead: false,
        }));
    };
    let on_move = move |e: web_sys::PointerEvent| {
        let Some(mut p) = pending.get_value() else {
            return;
        };
        if p.dead || e.pointer_id() != p.pointer {
            return;
        }
        let dy = e.client_y() as f64 - p.y0;
        if !p.active {
            if dy.abs() <= 4.0 {
                return;
            }
            p.active = true;
            if let Some(c) = list_ref.get_untracked() {
                let _ = c.set_pointer_capture(p.pointer);
            }
            pending.set_value(Some(p.clone()));
        }
        let c = &p.centers;
        let here = c[p.source];
        let dy = dy.clamp(c[0] - here, c[c.len() - 1] - here);
        let target = c
            .iter()
            .enumerate()
            .min_by(|a, b| {
                (a.1 - (here + dy))
                    .abs()
                    .total_cmp(&(b.1 - (here + dy)).abs())
            })
            .map_or(p.source, |(i, _)| i);
        let pitch = if c.len() > 1 {
            (c[c.len() - 1] - c[0]) / (c.len() - 1) as f64
        } else {
            0.0
        };
        drag.set(Some(Drag {
            id: p.id,
            source: p.source,
            target,
            dy,
            pitch,
        }));
    };
    let on_up = move |e: web_sys::PointerEvent| {
        let Some(p) = pending.get_value() else { return };
        if e.pointer_id() != p.pointer {
            return;
        }
        pending.set_value(None);
        if let Some(c) = list_ref.get_untracked() {
            let _ = c.release_pointer_capture(p.pointer);
        }
        if p.active {
            if p.dead {
                suppress.set_value(true);
                set_timeout(move || suppress.set_value(false), Duration::from_millis(50));
            } else {
                finish(true);
            }
        }
    };
    let on_cancel = move |_: web_sys::PointerEvent| {
        pending.set_value(None);
        finish(false);
    };
    let on_lost = move |_: web_sys::PointerEvent| {
        if pending.get_value().is_some_and(|p| p.active) {
            pending.set_value(None);
            finish(false);
        }
    };
    window_event_listener(leptos::ev::keydown, move |e| {
        if e.key() == "Escape"
            && let Some(mut p) = pending.get_value()
            && p.active
            && !p.dead
        {
            p.dead = true;
            pending.set_value(Some(p));
            finish(false);
        }
    });
    view! {
        <nav class="sidebar">
            <div
                class="accounts"
                node_ref=list_ref
                class:dragging=move || drag.get().is_some()
                on:pointerdown=on_down
                on:pointermove=on_move
                on:pointerup=on_up
                on:pointercancel=on_cancel
                on:lostpointercapture=on_lost
                on:dragstart=|e: web_sys::DragEvent| e.prevent_default()
            >
                <For
                    each=move || order.get()
                    key=|id| *id
                    children=move |id| {
                        let acct = Memo::new(move |_| snap.with(|s| s.accounts.iter().find(|a| a.id == id).cloned()));
                        let active = move || snap.with(|s| s.active == Some(id));
                        view! {
                            <button
                                class="acct"
                                class:active=active
                                class:muted=move || acct.get().is_some_and(|a| a.settings.muted)
                                class:disabled=move || acct.get().is_some_and(|a| !a.settings.enabled)
                                class:hibernated=move || acct.get().is_some_and(|a| a.settings.enabled && a.hibernated)
                                title=move || acct.get().map(|a| format!("{} \u{2013} {}", a.service_name, a.settings.label)).unwrap_or_default()
                                data-id=id.to_string()
                                draggable="false"
                                class:dragged=move || drag.get().is_some_and(|d| d.id == id)
                                style=move || {
                                    let idx = order.with(|o| o.iter().position(|x| *x == id)).unwrap_or(0);
                                    drag_style(drag.get(), id, idx)
                                }
                                on:click=move |_| {
                                    if suppress.get_value() {
                                        suppress.set_value(false);
                                    } else {
                                        cmd(UiCommand::Select { account: id });
                                    }
                                }
                            >
                                {move || acct.get().map(|a| {
                                    let show_badge = a.settings.show_badge;
                                    let muted = a.settings.muted;
                                    let hibernated = a.settings.enabled && a.hibernated;
                                    view! {
                                        {icon_view(&a, false)}
                                        {(show_badge && a.unread > 0).then(|| view! { <span class="badge">{badge_text(a.unread)}</span> })}
                                        {(show_badge && a.unread == 0 && a.indirect).then(|| view! { <span class="dot"></span> })}
                                        {muted.then(|| view! { <span class="mute-mark">"\u{1F507}"</span> })}
                                        {hibernated.then(|| view! { <span class="moon-mark" title="Sleeping">"\u{1F319}"</span> })}
                                    }
                                })}
                            </button>
                        }
                    }
                />
            </div>
            <div class="bottom">
                <button class="tool" title="Add account" on:click=move |_| open_dialog(Tab::Accounts, true)><Icon svg=ICON_PLUS /></button>
                <button class="tool" title="Settings" on:click=move |_| open_dialog(Tab::General, false)><Icon svg=ICON_GEAR /></button>
                <button
                    class="tool"
                    class:on=move || dnd.get()
                    title=move || if dnd.get() { "Do not disturb: on" } else { "Do not disturb: off" }
                    on:click=move |_| cmd(UiCommand::SetDnd { enabled: !dnd.get_untracked() })
                >
                    {move || view! { <Icon svg=if dnd.get() { ICON_BELL_OFF } else { ICON_BELL } /> }}
                </button>
            </div>
        </nav>
    }
}

/// Round icon: the account's image if it has one, else initials on the service color.
fn icon_view(a: &AccountView, small: bool) -> AnyView {
    match &a.icon {
        Some(src) if a.icon_glyph => view! { <span class="icon has-img glyph" class:small=small style=format!("background:{}", a.color)><img src=src.clone() alt="" draggable="false" /></span> }.into_any(),
        Some(src) => view! { <span class="icon has-img" class:small=small><img src=src.clone() alt="" draggable="false" /></span> }.into_any(),
        None => view! { <span class="icon" class:small=small style=format!("background:{}", a.color)>{a.initials.clone()}</span> }.into_any(),
    }
}

const ICON_PLUS: &str = "<svg viewBox='0 0 24 24' width='20' height='20' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='M12 5v14M5 12h14'/></svg>";
const ICON_GEAR: &str = "<svg viewBox='0 0 24 24' width='20' height='20' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><circle cx='12' cy='12' r='3'/><path d='M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z'/></svg>";
const ICON_BELL: &str = "<svg viewBox='0 0 24 24' width='20' height='20' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9'/><path d='M13.73 21a2 2 0 0 1-3.46 0'/></svg>";
const ICON_BELL_OFF: &str = "<svg viewBox='0 0 24 24' width='20' height='20' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='M13.73 21a2 2 0 0 1-3.46 0'/><path d='M18.63 13A17.89 17.89 0 0 1 18 8'/><path d='M6.26 6.26A5.86 5.86 0 0 0 6 8c0 7-3 9-3 9h14'/><path d='M18 8a6 6 0 0 0-9.33-5'/><path d='M1 1l22 22'/></svg>";
const ICON_UP: &str = "<svg viewBox='0 0 24 24' width='16' height='16' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='M12 19V5M5 12l7-7 7 7'/></svg>";
const ICON_DOWN: &str = "<svg viewBox='0 0 24 24' width='16' height='16' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='M12 5v14M19 12l-7 7-7-7'/></svg>";
const ICON_RELOAD: &str = "<svg viewBox='0 0 24 24' width='16' height='16' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='M21 12a9 9 0 1 1-3-6.7'/><path d='M21 4v5h-5'/></svg>";
const ICON_BACK: &str = "<svg viewBox='0 0 24 24' width='16' height='16' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='M19 12H5M12 19l-7-7 7-7'/></svg>";
const ICON_FWD: &str = "<svg viewBox='0 0 24 24' width='16' height='16' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='M5 12h14M12 5l7 7-7 7'/></svg>";

/// Directive: switch off macOS auto-correction (not a typed attribute in leptos).
fn noac(el: web_sys::Element) {
    let _ = el.set_attribute("autocorrect", "off");
}

/// Inline SVG drawn with `currentColor`, centred in a fixed-size box.
#[component]
fn Icon(svg: &'static str) -> impl IntoView {
    view! { <span class="ico" inner_html=svg></span> }
}

/// Two-step destructive action: the first click arms it ("Confirm ..." + "Cancel"), which
/// reverts by itself after 5 s. Replaces `window.confirm`, which WKWebView does not support.
#[component]
fn ConfirmButton(
    label: &'static str,
    confirm_label: &'static str,
    #[prop(into)] on_confirm: Callback<()>,
    #[prop(optional)] small: bool,
    #[prop(optional, into)] disabled: Signal<bool>,
    #[prop(optional, into)] title: Signal<String>,
) -> impl IntoView {
    let armed = RwSignal::new(false);
    let generation = RwSignal::new(0u32);
    let size = if small { " small" } else { "" };
    let arm = move |_| {
        armed.set(true);
        let g = generation.get_untracked() + 1;
        generation.set(g);
        set_timeout(
            move || {
                if generation.try_get_untracked() == Some(g) {
                    armed.try_set(false);
                }
            },
            Duration::from_secs(5),
        );
    };
    view! {
        <Show
            when=move || armed.get()
            fallback=move || view! {
                <button type="button" class=format!("btn danger{size}") disabled=move || disabled.get() title=move || title.get() on:click=arm>{label}</button>
            }
        >
            <button type="button" class=format!("btn danger confirm{size}") on:click=move |_| {
                armed.set(false);
                on_confirm.run(());
            }>{confirm_label}</button>
            <button type="button" class=format!("btn{size}") on:click=move |_| armed.set(false)>"Cancel"</button>
        </Show>
    }
}

/// One form row: label column + control column (with optional muted helper text under it).
#[component]
fn Row(
    #[prop(into)] label: String,
    #[prop(optional)] hint: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="frow">
            <span class="flabel">{label}</span>
            <div class="fctl">
                {children()}
                {(!hint.is_empty()).then(|| view! { <span class="hint">{hint}</span> })}
            </div>
        </div>
    }
}

/// Apply `f` to a copy of the current settings and send it.
fn update_settings(snap: ReadSignal<Snapshot>, f: impl FnOnce(&mut AppSettings)) {
    let mut s = snap.with_untracked(|s| s.settings.clone());
    f(&mut s);
    cmd(UiCommand::UpdateSettings { settings: s });
}

#[component]
fn SettingsDialog(snap: ReadSignal<Snapshot>) -> impl IntoView {
    let ui = expect_context::<UiState>();
    view! {
        <div class="dialog">
            <aside class="tabs">
                <h1>"Settings"</h1>
                {Tab::ALL.map(|t| view! {
                    <button class="tab" class:active=move || ui.tab.get() == t on:click=move |_| ui.tab.set(t)>{t.title()}</button>
                }).into_iter().collect_view()}
                <span class="spacer"></span>
                <button class="btn" on:click=move |_| cmd(UiCommand::SetDialog { open: false })>"Close"</button>
            </aside>
            <main class="content">
                {move || match ui.tab.get() {
                    Tab::Accounts => view! { <AccountsTab snap=snap /> }.into_any(),
                    Tab::Custom => view! { <CustomTab snap=snap /> }.into_any(),
                    Tab::General => view! { <GeneralTab snap=snap /> }.into_any(),
                    Tab::Notifications => view! { <NotificationsTab snap=snap /> }.into_any(),
                    Tab::Dock => view! { <DockTab snap=snap /> }.into_any(),
                    Tab::Links => view! { <LinksTab snap=snap /> }.into_any(),
                    Tab::About => view! { <AboutTab snap=snap /> }.into_any(),
                }}
            </main>
        </div>
    }
}

/// macOS-style switch row.
#[component]
fn Toggle(
    label: &'static str,
    #[prop(optional)] hint: &'static str,
    #[prop(into)] checked: Signal<bool>,
    #[prop(into)] on_change: Callback<bool>,
) -> impl IntoView {
    view! {
        <label class="frow toggle">
            <span class="flabel">{label}</span>
            <span class="fctl">
                <input type="checkbox" class="switch" prop:checked=move || checked.get() on:change=move |e| on_change.run(event_target_checked(&e)) />
                {(!hint.is_empty()).then(|| view! { <span class="hint">{hint}</span> })}
            </span>
        </label>
    }
}

fn app_toggle(
    snap: ReadSignal<Snapshot>,
    label: &'static str,
    hint: &'static str,
    get: fn(&AppSettings) -> bool,
    set: fn(&mut AppSettings, bool),
) -> impl IntoView {
    let value = Memo::new(move |_| snap.with(|s| get(&s.settings)));
    view! {
        <Toggle label=label hint=hint checked=value on_change=move |v| update_settings(snap, |s| set(s, v)) />
    }
}

#[component]
fn GeneralTab(snap: ReadSignal<Snapshot>) -> impl IntoView {
    let size = Memo::new(move |_| snap.with(|s| s.settings.icon_size));
    let opt = move |v: IconSize, label: &'static str| {
        view! {
            <button class="btn" class:on=move || size.get() == v on:click=move |_| update_settings(snap, |s| s.icon_size = v)>{label}</button>
        }
    };
    view! {
        <h2>"General"</h2>
        <div class="form">
            <Row label="Sidebar icon size" hint="Size of the account icons; the sidebar width follows.">
                <div class="btn-group seg">
                    {opt(IconSize::Small, "Small")}
                    {opt(IconSize::Medium, "Medium")}
                    {opt(IconSize::Large, "Large")}
                </div>
            </Row>
            {app_toggle(snap, "Launch at login", "", |s| s.launch_at_login, |s, v| s.launch_at_login = v)}
            {app_toggle(snap, "Start hidden", "Start without showing the window.", |s| s.start_hidden, |s, v| s.start_hidden = v)}
            {app_toggle(snap, "Keep running in background", "Otherwise closing the window quits the app.", |s| s.close_to_background, |s, v| s.close_to_background = v)}
            {app_toggle(snap, "Menu bar icon", "Unread count and account menu in the menu bar.", |s| s.show_tray, |s, v| s.show_tray = v)}
            <Row label="Put inactive services to sleep after" hint="Sleeping services free memory but show no notifications or unread counts until you open them again. Selecting a service wakes it up.">
                <select prop:value=move || snap.with(|s| s.settings.hibernate_after_min.filter(|&m| m > 0).map(|m| m.to_string()).unwrap_or_default())
                    on:change=move |e| { let v = event_target_value(&e).parse::<u32>().ok(); update_settings(snap, |s| s.hibernate_after_min = v) }>
                    <option value="">"Never"</option>
                    {move || num_options(&HIBERNATE, snap.with(|s| s.settings.hibernate_after_min), minutes_label)}
                </select>
            </Row>
            {app_toggle(snap, "Reload after wake", "Reload services after waking from sleep; fixes stale connections.", |s| s.reload_after_wake, |s, v| s.reload_after_wake = v)}
        </div>
    }
}

#[component]
fn NotificationsTab(snap: ReadSignal<Snapshot>) -> impl IntoView {
    view! {
        <h2>"Notifications"</h2>
        <div class="form">
            {app_toggle(snap, "Do not disturb", "Suppress all notifications.", |s| s.dnd, |s, v| s.dnd = v)}
            {app_toggle(snap, "Message preview", "Otherwise only \u{201C}New message\u{201D} is shown.", |s| s.notification_preview, |s, v| s.notification_preview = v)}
            {app_toggle(snap, "Play sound", "", |s| s.notification_sound, |s, v| s.notification_sound = v)}
        </div>
    }
}

#[component]
fn DockTab(snap: ReadSignal<Snapshot>) -> impl IntoView {
    let mode = Memo::new(move |_| snap.with(|s| s.settings.dock_badge));
    let opt = move |m: DockBadgeMode, label: &'static str| {
        view! {
            <label class="radio">
                <input type="radio" name="dock-badge" prop:checked=move || mode.get() == m
                    on:change=move |_| update_settings(snap, |s| s.dock_badge = m) />
                <span>{label}</span>
            </label>
        }
    };
    view! {
        <h2>"Dock badge"</h2>
        <div class="form">
            <Row label="Show" hint="Accounts can be excluded individually in their settings (Accounts tab).">
                {opt(DockBadgeMode::All, "Unread count and dot")}
                {opt(DockBadgeMode::CountOnly, "Unread count only")}
                {opt(DockBadgeMode::Off, "Off")}
            </Row>
        </div>
    }
}

const BROWSERS: [&str; 6] = [
    "Safari",
    "Firefox",
    "Google Chrome",
    "Arc",
    "Microsoft Edge",
    "Brave Browser",
];

#[component]
fn LinksTab(snap: ReadSignal<Snapshot>) -> impl IntoView {
    let browser = Memo::new(move |_| snap.with(|s| s.settings.browser.clone()));
    // Draft of the text field: committed on change/blur, never overwritten by snapshots.
    let draft = RwSignal::new(browser.get_untracked().unwrap_or_default());
    let commit = move || {
        let v = draft.get_untracked().trim().to_string();
        update_settings(snap, |s| s.browser = (!v.is_empty()).then_some(v));
    };
    let target = Memo::new(move |_| snap.with(|s| s.settings.link_target));
    let target_opt = move |t: LinkTarget, label: &'static str| {
        view! {
            <label class="radio">
                <input type="radio" name="link-target" prop:checked=move || target.get() == t
                    on:change=move |_| update_settings(snap, |s| s.link_target = t) />
                <span>{label}</span>
            </label>
        }
    };
    view! {
           <h2>"Links"</h2>
           <div class="form">
               <Row label="New-window links" hint="Sign-in popups always stay in the app, so OAuth and SSO keep working.">
                   {target_opt(LinkTarget::SystemBrowser, "System browser")}
                   {target_opt(LinkTarget::AppWindow, "App window")}
               </Row>
               <Row label="Browser" hint="Enter the name of an installed application, e.g. Firefox.">
                   <label class="radio">
                       <input type="radio" name="browser" prop:checked=move || browser.get().is_none()
                           on:change=move |_| update_settings(snap, |s| s.browser = None) />
                       <span>"System default browser"</span>
                   </label>
                   <label class="radio">
                       <input type="radio" name="browser" prop:checked=move || browser.get().is_some()
                           on:change=move |_| {
                               if draft.get_untracked().trim().is_empty() {
                                   draft.set("Safari".into());
                               }
                               commit();
                           } />
                       <span>"Open links in:"</span>
                       <input
                           type="text"
                           list="browsers"
                           placeholder="Application name"
                           autocapitalize="off"
                           spellcheck="false"
                           autocomplete="off"
                           prop:value=move || draft.get()
                           on:input=move |e| draft.set(event_target_value(&e))
                           on:change=move |_| commit()
    use:noac />
                       <datalist id="browsers">
                           {BROWSERS.map(|b| view! { <option value=b></option> }).into_iter().collect_view()}
                       </datalist>
                   </label>
               </Row>
           </div>
       }
}

#[component]
fn AboutTab(snap: ReadSignal<Snapshot>) -> impl IntoView {
    let app = Memo::new(move |_| snap.with(|s| s.app.clone()));
    view! {
        <h2>"About"</h2>
        <div class="form">
            <Row label="Application"><code>"Messengers"</code></Row>
            <Row label="Version"><code>{move || app.get().version}</code></Row>
            <Row label="Config directory"><code>{move || app.get().config_dir}</code></Row>
            <Row label="Log file"><code>{move || app.get().log_file}</code></Row>
        </div>
    }
}

fn focus_by_id(id: &'static str) {
    request_animation_frame(move || {
        if let Some(el) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(id))
            .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = el.focus();
        }
    });
}

const CUSTOM_OPTION: &str = "__custom__";

#[component]
fn AccountsTab(snap: ReadSignal<Snapshot>) -> impl IntoView {
    let ui = expect_context::<UiState>();
    let service = RwSignal::new(String::new());
    let label = RwSignal::new(String::new());
    let team = RwSignal::new(String::new());
    // Which account is being edited; its draft lives inside `EditForm`.
    let initial_edit = ui
        .edit_idx
        .get_untracked()
        .and_then(|i| snap.with_untracked(|s| s.accounts.get(i).map(|a| a.id)));
    ui.edit_idx.set(None);
    let editing: RwSignal<Option<Uuid>> = RwSignal::new(initial_edit);

    Effect::new(move |_| {
        if ui.focus_add.get() {
            ui.focus_add.set(false);
            focus_by_id("add-service");
        }
    });

    let services = Memo::new(move |_| snap.with(|s| s.services.clone()));
    let current = move || {
        let id = service.get();
        services.with(|l| l.iter().find(|x| x.id == id).or(l.first()).cloned())
    };
    // Reset the team field only when the effective service changes.
    let current_id = Memo::new(move |_| current().map(|s| s.id));
    Effect::new(move |_| {
        if current_id.get().is_some() {
            team.set(current().and_then(|s| s.team_default).unwrap_or_default());
        }
    });

    let added: RwSignal<Option<String>> = RwSignal::new(None);
    let add = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let Some(sv) = current() else { return };
        let team_v = team.get_untracked().trim().to_string();
        let team_opt = (sv.team_label.is_some() && !team_v.is_empty()).then_some(team_v);
        let l = label.get_untracked().trim().to_string();
        let l = if l.is_empty() { sv.name.clone() } else { l };
        cmd(UiCommand::AddAccount {
            service: sv.id.clone(),
            label: l.clone(),
            team: team_opt,
        });
        label.set(String::new());
        team.set(sv.team_default.clone().unwrap_or_default());
        added.set(Some(l.clone()));
        set_timeout(
            move || {
                if added.get_untracked().as_deref() == Some(l.as_str()) {
                    added.set(None);
                }
            },
            Duration::from_secs(3),
        );
        focus_by_id("add-label");
    };

    let ids =
        Memo::new(move |_| snap.with(|s| s.accounts.iter().map(|a| a.id).collect::<Vec<_>>()));

    view! {
        <h2>"Accounts"</h2>
        <h3>"Add account"</h3>
        <form id="add-form" class="form" novalidate on:submit=add>
            <Row label="Service">
                <select
                    id="add-service"
                    prop:value=move || current().map(|s| s.id).unwrap_or_default()
                    on:change=move |e| {
                        let v = event_target_value(&e);
                        if v == CUSTOM_OPTION {
                            // Restore the previous selection and jump to the custom services tab.
                            if let Some(id) = current().map(|s| s.id) {
                                event_target::<web_sys::HtmlSelectElement>(&e).set_value(&id);
                            }
                            ui.tab.set(Tab::Custom);
                        } else {
                            service.set(v);
                        }
                    }
                >
                    <For each=move || services.get() key=|s| s.id.clone() children=|s| {
                        let name = if s.custom { format!("{} (custom)", s.name) } else { s.name.clone() };
                        view! { <option value=s.id.clone()>{name}</option> }
                    } />
                    <option value=CUSTOM_OPTION>"+ Custom app or site\u{2026}"</option>
                </select>
            </Row>
            <Row label="Label">
                <input
                    id="add-label"
                    type="text"
                    spellcheck="false"
                    placeholder=move || current().map(|s| s.name).unwrap_or_default()
                    prop:value=move || label.get()
                    on:input=move |e| label.set(event_target_value(&e))
                />
            </Row>
            {move || current().and_then(|s| s.team_label).map(|tl| view! {
                <Row label=tl>
                    <input type="text" autocapitalize="off" spellcheck="false" autocomplete="off"
                        prop:value=move || team.get() on:input=move |e| team.set(event_target_value(&e)) use:noac />
                </Row>
            })}
            <Row label="">
                <button class="btn primary" type="submit" disabled=move || current().is_none()>"Add"</button>
                {move || added.get().map(|l| view! { <span class="added-note">{format!("Added {l}")}</span> })}
            </Row>
        </form>
        <h3 class="spaced">"Your accounts"</h3>
        <section class="list">
            <Show
                when=move || !ids.with(|i| i.is_empty())
                fallback=|| view! { <p class="empty">"No accounts yet. Use the \u{201C}Add account\u{201D} form above to add your first one."</p> }
            >
                <For
                    each=move || ids.get()
                    key=|id| *id
                    children=move |id| view! { <AccountRow snap=snap id=id editing=editing /> }
                />
            </Show>
        </section>
    }
}

#[component]
fn AccountRow(
    snap: ReadSignal<Snapshot>,
    id: Uuid,
    editing: RwSignal<Option<Uuid>>,
) -> impl IntoView {
    let acct = Memo::new(move |_| snap.with(|s| s.accounts.iter().find(|a| a.id == id).cloned()));
    view! {
        {move || acct.get().map(|a| {
            let is_editing = move || editing.get() == Some(id);
            let a_edit = a.clone();
            let enabled = a.settings.enabled;
            let global_hibernate = snap.with_untracked(|s| s.settings.hibernate_after_min);
            let media_default = snap.with_untracked(|s| s.services.iter().find(|x| x.id == a.service).is_some_and(|x| x.allow_media));
            view! {
                <div class="acct-item" class:open=is_editing>
                    <div class="row">
                        <button class="row-main" title="Edit account" on:click=move |_| {
                            editing.set(if editing.get_untracked() == Some(id) { None } else { Some(id) })
                        }>
                            {icon_view(&a, true)}
                            <span class="names"><strong>{a.service_name.clone()}</strong><span class="sub">{a.settings.label.clone()}</span></span>
                            {a.settings.muted.then(|| view! { <span class="sub">"\u{1F507}"</span> })}
                        </button>
                        <div class="btn-group">
                            <button class="btn icon-btn" title="Move up" on:click=move |_| cmd(UiCommand::Move { account: id, delta: -1 })><Icon svg=ICON_UP /></button>
                            <button class="btn icon-btn" title="Move down" on:click=move |_| cmd(UiCommand::Move { account: id, delta: 1 })><Icon svg=ICON_DOWN /></button>
                            <button class="btn icon-btn" title="Reload" on:click=move |_| cmd(UiCommand::Reload { account: id })><Icon svg=ICON_RELOAD /></button>
                        </div>
                        <ConfirmButton label="Remove" confirm_label="Confirm remove" on_confirm=move |_| cmd(UiCommand::RemoveAccount { account: id }) />
                    </div>
                    <div class="row actions-row">
                        <button class="btn small" title="Open the current page in the browser" on:click=move |_| cmd(UiCommand::OpenInBrowser { account: id })>"Open in browser"</button>
                        <div class="btn-group">
                            <button class="btn small icon-btn" title="Back" on:click=move |_| cmd(UiCommand::GoBack { account: id })><Icon svg=ICON_BACK /></button>
                            <button class="btn small icon-btn" title="Forward" on:click=move |_| cmd(UiCommand::GoForward { account: id })><Icon svg=ICON_FWD /></button>
                        </div>
                        <button class="btn small" title="Unload the view to save memory" disabled=!enabled on:click=move |_| cmd(UiCommand::Hibernate { account: id })>"Sleep now"</button>
                        <ConfirmButton small=true label="Clear website data\u{2026}" confirm_label="Confirm: log out" title="Logs you out of this service" on_confirm=move |_| cmd(UiCommand::ClearData { account: id }) />
                    </div>
                    {move || is_editing().then(|| view! { <EditForm a=a_edit.clone() editing=editing media_default=media_default global_hibernate=global_hibernate /> })}
                </div>
            }
        })}
    }
}

const MAX_ICON_BYTES: f64 = 256.0 * 1024.0;
const ZOOMS: [u32; 10] = [50, 67, 80, 90, 100, 110, 125, 150, 175, 200];
const HIBERNATE: [u32; 5] = [5, 15, 30, 60, 120];

/// "5 min", "2 h", ...
fn minutes_label(n: u32) -> String {
    if n >= 60 && n.is_multiple_of(60) {
        format!("{} h", n / 60)
    } else {
        format!("{n} min")
    }
}

/// Label of a global sleep timeout.
fn hibernate_label(v: Option<u32>) -> String {
    v.filter(|&m| m > 0)
        .map_or_else(|| "Never".to_string(), minutes_label)
}

/// Options of a numeric select; a stored value that is not in the list is kept as an extra option.
fn num_options(list: &[u32], current: Option<u32>, fmt: fn(u32) -> String) -> impl IntoView {
    let mut v = list.to_vec();
    if let Some(c) = current
        && c > 0
        && !v.contains(&c)
    {
        v.push(c);
        v.sort_unstable();
    }
    v.into_iter()
        .map(|n| view! { <option value=n.to_string()>{fmt(n)}</option> })
        .collect_view()
}

/// Read the chosen image file as a data URL into `icon`; files over 256 KB are rejected.
fn read_image(
    input: web_sys::HtmlInputElement,
    icon: RwSignal<Option<String>>,
    err: RwSignal<String>,
) {
    err.set(String::new());
    let Some(file) = input.files().and_then(|f| f.get(0)) else {
        return;
    };
    if file.size() > MAX_ICON_BYTES {
        err.set(format!(
            "Image is too large ({} KB); the limit is 256 KB.",
            (file.size() / 1024.0).ceil()
        ));
        input.set_value("");
        return;
    }
    let Ok(reader) = web_sys::FileReader::new() else {
        return;
    };
    let r2 = reader.clone();
    let onload = Closure::once_into_js(move || {
        if let Ok(v) = r2.result()
            && let Some(s) = v.as_string()
        {
            icon.set(Some(s));
        }
    });
    reader.set_onload(Some(onload.unchecked_ref()));
    if reader.read_as_data_url(&file).is_err() {
        err.set("Could not read the image.".into());
    }
    input.set_value("");
}

/// Split a stored user agent into (select mode, custom text).
fn ua_split(ua: Option<&str>, none_mode: &'static str) -> (String, String) {
    match ua {
        None => (none_mode.to_string(), String::new()),
        Some(m @ ("safari" | "chrome" | "default")) => (m.to_string(), String::new()),
        Some(other) => ("custom".to_string(), other.to_string()),
    }
}

/// Inline edit form. The draft is copied from the account once, when the form opens; later
/// snapshots do not touch it.
#[component]
fn EditForm(
    a: AccountView,
    editing: RwSignal<Option<Uuid>>,
    media_default: bool,
    global_hibernate: Option<u32>,
) -> impl IntoView {
    let id = a.id;
    let service_name = a.service_name.clone();
    let s = a.settings;
    let (mode, custom) = ua_split(s.user_agent.as_deref(), "");
    let site_icon = if s.custom_icon.is_none() && !a.icon_glyph {
        a.icon.clone()
    } else {
        None
    };
    let glyph_icon = if s.custom_icon.is_none() && a.icon_glyph {
        a.icon.clone()
    } else {
        None
    };
    let (color, initials) = (a.color.clone(), a.initials.clone());
    let label = RwSignal::new(s.label);
    let team = RwSignal::new(s.team.unwrap_or_default());
    let notif = RwSignal::new(!s.muted);
    let badge = RwSignal::new(s.show_badge);
    let dock = RwSignal::new(s.in_dock_badge);
    let enabled = RwSignal::new(s.enabled);
    let links = RwSignal::new(match s.link_target {
        None => "",
        Some(LinkTarget::SystemBrowser) => "system",
        Some(LinkTarget::AppWindow) => "app",
    });
    let icon: RwSignal<Option<String>> = RwSignal::new(s.custom_icon.clone());
    let icon_url = RwSignal::new(
        s.custom_icon
            .filter(|i| !i.starts_with("data:"))
            .unwrap_or_default(),
    );
    let icon_err = RwSignal::new(String::new());
    let simple = RwSignal::new(s.simple_icon.unwrap_or_default());
    let zoom_pct = (s.zoom * 100.0).round().max(1.0) as u32;
    let zoom = RwSignal::new(zoom_pct.to_string());
    let hib = RwSignal::new(
        s.hibernate_after_min
            .map(|m| m.to_string())
            .unwrap_or_default(),
    );
    let css = RwSignal::new(s.custom_css.unwrap_or_default());
    let app_hosts = RwSignal::new(s.app_hosts.join("\n"));
    let media = RwSignal::new(match s.allow_media {
        None => "",
        Some(true) => "allow",
        Some(false) => "block",
    });
    let ua_mode = RwSignal::new(mode);
    let ua_custom = RwSignal::new(custom);
    let team_label = a.team_label;
    let has_team = team_label.is_some();
    let zoom_opts = num_options(&ZOOMS, Some(zoom_pct), |n| format!("{n} %"));
    let hib_opts = num_options(&HIBERNATE, s.hibernate_after_min, minutes_label);

    let save = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let team_v = team.get_untracked().trim().to_string();
        let user_agent = match ua_mode.get_untracked().as_str() {
            "" => None,
            "custom" => {
                Some(ua_custom.get_untracked().trim().to_string()).filter(|v| !v.is_empty())
            }
            m => Some(m.to_string()),
        };
        let l = label.get_untracked().trim().to_string();
        let css_v = css.get_untracked();
        cmd(UiCommand::UpdateAccount {
            account: id,
            settings: AccountSettings {
                label: if l.is_empty() {
                    service_name.clone()
                } else {
                    l
                },
                team: (has_team && !team_v.is_empty()).then_some(team_v),
                muted: !notif.get_untracked(),
                show_badge: badge.get_untracked(),
                in_dock_badge: dock.get_untracked(),
                user_agent,
                enabled: enabled.get_untracked(),
                link_target: match links.get_untracked() {
                    "system" => Some(LinkTarget::SystemBrowser),
                    "app" => Some(LinkTarget::AppWindow),
                    _ => None,
                },
                custom_icon: icon.get_untracked().filter(|i| !i.is_empty()),
                simple_icon: Some(simple.get_untracked().trim().to_string())
                    .filter(|v| !v.is_empty()),
                zoom: zoom
                    .get_untracked()
                    .parse::<f64>()
                    .map_or(1.0, |p| p / 100.0),
                hibernate_after_min: hib.get_untracked().parse().ok(),
                custom_css: (!css_v.trim().is_empty()).then_some(css_v),
                allow_media: match media.get_untracked() {
                    "allow" => Some(true),
                    "block" => Some(false),
                    _ => None,
                },
                app_hosts: app_hosts
                    .get_untracked()
                    .lines()
                    .map(|h| h.trim().to_string())
                    .filter(|h| !h.is_empty())
                    .collect(),
            },
        });
        editing.set(None);
    };

    view! {
        <form class="edit form" novalidate on:submit=save>
            <Row label="Label">
                <input type="text" spellcheck="false" prop:value=move || label.get() on:input=move |e| label.set(event_target_value(&e)) />
            </Row>
            {team_label.map(|tl| view! {
                <Row label=tl hint="Changing the instance reloads the account.">
                    <input type="text" autocapitalize="off" spellcheck="false" autocomplete="off"
                        prop:value=move || team.get() on:input=move |e| team.set(event_target_value(&e)) use:noac />
                </Row>
            })}
            <Toggle label="Enabled" hint="Disabled accounts keep their login but are not loaded." checked=enabled on_change=move |v| enabled.set(v) />
            <Toggle label="Notifications" checked=notif on_change=move |v| notif.set(v) />
            <Toggle label="Unread badge" checked=badge on_change=move |v| badge.set(v) />
            <Toggle label="Count in dock badge" checked=dock on_change=move |v| dock.set(v) />
            <Row label="Open links in">
                <select prop:value=move || links.get() on:change=move |e| links.set(match event_target_value(&e).as_str() {
                    "system" => "system",
                    "app" => "app",
                    _ => "",
                })>
                    <option value="">"Use global setting"</option>
                    <option value="system">"System browser"</option>
                    <option value="app">"App window"</option>
                </select>
            </Row>
            <Row label="Icon">
                <div class="icon-row">
                    {move || match icon.get() {
                        Some(src) => view! { <span class="icon small has-img"><img src=src alt="" /></span> }.into_any(),
                        None => match glyph_icon.clone().filter(|_| !simple.get().trim().is_empty()).or_else(|| site_icon.clone()) {
                        Some(src) if site_icon.as_ref() != Some(&src) => view! { <span class="icon small has-img glyph" style=format!("background:{color}")><img src=src alt="" /></span> }.into_any(),
                        Some(src) => view! { <span class="icon small has-img"><img src=src alt="" /></span> }.into_any(),
                        None => view! { <span class="icon small" style=format!("background:{color}")>{initials.clone()}</span> }.into_any(),
                    }}}
                    <div class="icon-actions">
                        <label class="btn small file-btn">
                            "Choose image\u{2026}"
                            <input type="file" accept="image/*" hidden
                                on:change=move |e| {
                                    icon_url.set(String::new());
                                    read_image(event_target::<web_sys::HtmlInputElement>(&e), icon, icon_err);
                                } />
                        </label>
                        <button class="btn small" type="button" on:click=move |_| { icon.set(None); simple.set(String::new()); icon_url.set(String::new()); icon_err.set(String::new()); }>"Use site icon"</button>
                        <button class="btn small" type="button" on:click=move |_| cmd(UiCommand::RefreshIcon { account: id })>"Refresh site icon"</button>
                    </div>
                </div>
                {move || { let e = icon_err.get(); (!e.is_empty()).then(|| view! { <span class="error">{e}</span> }) }}
            </Row>
            <Row label="Simple Icons" hint="Brand glyph on the service color (slug from simpleicons.org, letters and digits only). Precedence: custom image, Simple Icon, site icon, initials.">
                <input type="text" class="wide" placeholder="e.g. discord"
                    autocapitalize="off" spellcheck="false" autocomplete="off"
                    prop:value=move || simple.get() on:input=move |e| simple.set(event_target_value(&e)) use:noac />
            </Row>
            <Row label="Image URL">
                <input type="url" class="wide" placeholder="https://example.com/icon.png"
                    autocapitalize="off" spellcheck="false" autocomplete="off"
                    prop:value=move || icon_url.get()
                    on:input=move |e| icon_url.set(event_target_value(&e))
                    on:change=move |_| {
                        let v = icon_url.get_untracked().trim().to_string();
                        icon_err.set(String::new());
                        if !v.is_empty() {
                            icon.set(Some(v));
                        } else if icon.get_untracked().is_some_and(|i| !i.starts_with("data:")) {
                            icon.set(None);
                        }
                    } use:noac />
            </Row>
            <Row label="Camera & microphone">
                <select prop:value=move || media.get() on:change=move |e| media.set(match event_target_value(&e).as_str() {
                    "allow" => "allow",
                    "block" => "block",
                    _ => "",
                })>
                    <option value="">{if media_default { "Service default (Allowed)" } else { "Service default (Blocked)" }}</option>
                    <option value="allow">"Allow"</option>
                    <option value="block">"Block"</option>
                </select>
            </Row>
            <Row label="Zoom">
                <select prop:value=move || zoom.get() on:change=move |e| zoom.set(event_target_value(&e))>{zoom_opts}</select>
            </Row>
            <Row label="Sleep in background">
                <select prop:value=move || hib.get() on:change=move |e| hib.set(event_target_value(&e))>
                    <option value="">{format!("Use global setting ({})", hibernate_label(global_hibernate))}</option>
                    <option value="0">"Never"</option>
                    {hib_opts}
                </select>
            </Row>
            <Row label="User agent" hint="Changing the user agent reloads the account.">
                <select prop:value=move || ua_mode.get() on:change=move |e| ua_mode.set(event_target_value(&e))>
                    <option value="">"Service default"</option>
                    <option value="safari">"Safari"</option>
                    <option value="chrome">"Chrome"</option>
                    <option value="default">"System WebKit"</option>
                    <option value="custom">"Custom\u{2026}"</option>
                </select>
            </Row>
            {move || (ua_mode.get() == "custom").then(|| view! {
                <Row label="Custom user agent">
                    <input type="text" class="wide" autocapitalize="off" spellcheck="false" autocomplete="off"
                        prop:value=move || ua_custom.get() on:input=move |e| ua_custom.set(event_target_value(&e)) use:noac />
                </Row>
            })}
            <Row label="Always open in the app" hint="Links that open a new window to these hosts open inside the app with this account's login (e.g. a single-sign-on provider such as idp.example.org, or *.microsoftonline.com). Redirects within the page always stay in the app.">
                <textarea class="mono" rows="3" autocapitalize="off" spellcheck="false" autocomplete="off" placeholder="one host per line"
                    prop:value=move || app_hosts.get() on:input=move |e| app_hosts.set(event_target_value(&e)) use:noac ></textarea>
            </Row>
            <Row label="Custom CSS" hint="Injected into every page; changing it reloads the account.">
                <textarea class="mono" rows="6" autocapitalize="off" spellcheck="false" autocomplete="off" placeholder="/* injected into every page */"
                    prop:value=move || css.get() on:input=move |e| css.set(event_target_value(&e)) use:noac ></textarea>
            </Row>
            <Row label="">
                <div class="actions">
                    <button class="btn primary" type="submit">"Save"</button>
                    <button class="btn" type="button" on:click=move |_| editing.set(None)>"Cancel"</button>
                </div>
            </Row>
        </form>
    }
}

/// `None` = form closed, `Some(None)` = adding, `Some(Some(_))` = editing.
type CustomDraft = Option<Option<CustomService>>;

#[component]
fn CustomTab(snap: ReadSignal<Snapshot>) -> impl IntoView {
    let ui = expect_context::<UiState>();
    let initial: CustomDraft = ui.add_custom.get_untracked().then_some(None);
    ui.add_custom.set(false);
    let form: RwSignal<CustomDraft> = RwSignal::new(initial);
    let list = Memo::new(move |_| snap.with(|s| s.custom_services.clone()));
    view! {
        <h2>"Custom services"</h2>
        <p class="note">"Add any web app or site as a service. It then shows up in the service list when adding an account. A service can only be removed when no account uses it."</p>
        <Show
            when=move || !list.with(|l| l.is_empty())
            fallback=|| view! { <p class="empty">"No custom services yet."</p> }
        >
            <section class="list">
                <For
                    each=move || list.get()
                    key=|c| (c.id.clone(), c.name.clone(), c.url.clone(), c.color.clone(), c.team_label.clone(), c.title_regex.clone(), c.user_agent.clone())
                    children=move |c| {
                        let c_edit = c.clone();
                        let id = c.id.clone();
                        let id2 = c.id.clone();
                        let used = Memo::new(move |_| snap.with(|s| s.accounts.iter().filter(|a| a.service == id2).count()));
                        view! {
                            <div class="row">
                                <span class="icon small" style=format!("background:{}", c.color)>{c.name.chars().next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_default()}</span>
                                <span class="names"><strong>{c.name.clone()}</strong><span class="sub">{c.url.clone()}</span></span>
                                <button class="btn" on:click=move |_| form.set(Some(Some(c_edit.clone())))>"Edit"</button>
                                <ConfirmButton
                                    label="Remove"
                                    confirm_label="Confirm remove"
                                    disabled=Signal::derive(move || used.get() > 0)
                                    title=Signal::derive(move || match used.get() {
                                        0 => String::new(),
                                        n => format!("In use by {n} account(s)"),
                                    })
                                    on_confirm=move |_| cmd(UiCommand::RemoveCustomService { id: id.clone() })
                                />
                            </div>
                        }
                    }
                />
            </section>
        </Show>
        {move || match form.get() {
            None => view! { <button class="btn primary" on:click=move |_| form.set(Some(None))>"Add custom service\u{2026}"</button> }.into_any(),
            Some(existing) => view! { <CustomForm snap=snap existing=existing form=form /> }.into_any(),
        }}
        <h2>"Service definition files"</h2>
        <p class="note">"Edit or add TOML files here for full control (same format as the built-in services). A file with the same id as a built-in service overrides it. Click Reload to apply."</p>
        <div class="form">
            <Row label="Folder"><code>{move || snap.with(|s| s.app.services_dir.clone())}</code></Row>
            <Row label="">
                <div class="actions">
                    <button class="btn" on:click=move |_| cmd(UiCommand::OpenServicesFolder)>"Open folder"</button>
                    <button class="btn" on:click=move |_| cmd(UiCommand::ReloadServices)>"Reload"</button>
                </div>
            </Row>
        </div>
        {move || {
            let errors = snap.with(|s| s.service_errors.clone());
            (!errors.is_empty()).then(|| view! {
                <div class="note error">
                    <For each=move || errors.clone().into_iter().enumerate() key=|(i, e)| (*i, e.clone())
                        children=|(_, e)| view! { <div><code>{e}</code></div> } />
                </div>
            })
        }}
    }
}

#[component]
fn CustomForm(
    snap: ReadSignal<Snapshot>,
    existing: Option<CustomService>,
    form: RwSignal<CustomDraft>,
) -> impl IntoView {
    let editing = existing.is_some();
    let c = existing.unwrap_or_else(|| CustomService {
        id: String::new(),
        name: String::new(),
        url: String::new(),
        color: "#6366f1".into(),
        team_label: None,
        title_regex: None,
        user_agent: None,
    });
    let id = c.id.clone();
    let (mode, custom) = ua_split(c.user_agent.as_deref(), "safari");
    let color_ok = c.color.len() == 7 && c.color.starts_with('#');
    let name = RwSignal::new(c.name);
    let url = RwSignal::new(c.url);
    let color = RwSignal::new(if color_ok { c.color } else { "#888888".into() });
    let team_label = RwSignal::new(c.team_label.unwrap_or_default());
    let regex = RwSignal::new(c.title_regex.unwrap_or_default());
    let ua_mode = RwSignal::new(mode);
    let ua_custom = RwSignal::new(custom);
    let base = RwSignal::new(String::new());
    let builtin = Memo::new(move |_| {
        snap.with(|s| {
            s.services
                .iter()
                .filter(|x| !x.custom)
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let valid = move || !name.with(|n| n.trim().is_empty()) && !url.with(|u| u.trim().is_empty());
    let opt = |v: String| Some(v.trim().to_string()).filter(|v| !v.is_empty());

    // Prefill from a built-in service (adding only); "Blank" resets the fields.
    let prefill = move |id: String| {
        base.set(id.clone());
        let svc = builtin.with_untracked(|l| l.iter().find(|x| x.id == id).cloned());
        let (n, u, col, tl, re, ua) = match svc {
            Some(s) => (
                format!("{} (custom)", s.name),
                s.url,
                s.color,
                s.team_label,
                s.title_regex,
                s.user_agent,
            ),
            None => (
                String::new(),
                String::new(),
                "#6366f1".into(),
                None,
                None,
                None,
            ),
        };
        let (mode, custom) = ua_split(ua.as_deref(), "safari");
        name.set(n);
        url.set(u);
        color.set(if col.len() == 7 && col.starts_with('#') {
            col
        } else {
            "#888888".into()
        });
        team_label.set(tl.unwrap_or_default());
        regex.set(re.unwrap_or_default());
        ua_mode.set(mode);
        ua_custom.set(custom);
    };

    let save = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if !valid() {
            return;
        }
        let url_v = url.get_untracked().trim().to_string();
        let service = CustomService {
            id: id.clone(),
            name: name.get_untracked().trim().to_string(),
            color: color.get_untracked(),
            team_label: if url_v.contains("{team}") {
                opt(team_label.get_untracked())
            } else {
                None
            },
            url: url_v,
            title_regex: opt(regex.get_untracked()),
            user_agent: match ua_mode.get_untracked().as_str() {
                "safari" => None,
                "custom" => opt(ua_custom.get_untracked()),
                m => Some(m.to_string()),
            },
        };
        cmd(if editing {
            UiCommand::UpdateCustomService { service }
        } else {
            UiCommand::AddCustomService { service }
        });
        form.set(None);
    };

    view! {
        <form class="edit form custom-form" novalidate on:submit=save>
            <h3>{if editing { "Edit custom service" } else { "Add custom service" }}</h3>
            {(!editing).then(|| view! {
                <Row label="Start from" hint="Prefill the fields from a built-in service, then adjust them.">
                    <select prop:value=move || base.get() on:change=move |e| prefill(event_target_value(&e))>
                        <option value="">"Blank"</option>
                        <For each=move || builtin.get() key=|s| s.id.clone() children=|s| view! { <option value=s.id.clone()>{s.name.clone()}</option> } />
                    </select>
                </Row>
            })}
            <Row label="Name">
                <input type="text" spellcheck="false" prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e)) />
            </Row>
            <Row label="URL" hint="Use {team} for a per-account part, e.g. https://{team}/">
                <input type="url" class="wide" placeholder="https://example.com/"
                    autocapitalize="off" spellcheck="false" autocomplete="off"
                    prop:value=move || url.get() on:input=move |e| url.set(event_target_value(&e)) use:noac />
            </Row>
            <Row label="Color">
                <input type="color" prop:value=move || color.get() on:input=move |e| color.set(event_target_value(&e)) />
            </Row>
            {move || url.with(|u| u.contains("{team}")).then(|| view! {
                <Row label="Team field label" hint="Optional, e.g. Instance.">
                    <input type="text" placeholder="e.g. Instance" autocapitalize="off" spellcheck="false" autocomplete="off"
                        prop:value=move || team_label.get() on:input=move |e| team_label.set(event_target_value(&e)) use:noac />
                </Row>
            })}
            <Row label="Unread title pattern" hint="Optional regular expression on the page title; capture group 1 is the unread count.">
                <input type="text" class="wide mono" placeholder="^\\((\\d+)\\)"
                    autocapitalize="off" spellcheck="false" autocomplete="off"
                    prop:value=move || regex.get() on:input=move |e| regex.set(event_target_value(&e)) use:noac />
            </Row>
            <Row label="User agent">
                <select prop:value=move || ua_mode.get() on:change=move |e| ua_mode.set(event_target_value(&e))>
                    <option value="safari">"Safari (default)"</option>
                    <option value="chrome">"Chrome"</option>
                    <option value="default">"System WebKit"</option>
                    <option value="custom">"Custom\u{2026}"</option>
                </select>
            </Row>
            {move || (ua_mode.get() == "custom").then(|| view! {
                <Row label="Custom user agent">
                    <input type="text" class="wide" autocapitalize="off" spellcheck="false" autocomplete="off"
                        prop:value=move || ua_custom.get() on:input=move |e| ua_custom.set(event_target_value(&e)) use:noac />
                </Row>
            })}
            <Row label="">
                <div class="actions">
                    <button class="btn primary" type="submit" disabled=move || !valid()>{if editing { "Save" } else { "Add" }}</button>
                    <button class="btn" type="button" on:click=move |_| form.set(None)>"Cancel"</button>
                </div>
            </Row>
        </form>
    }
}

fn main() {
    mount_to_body(App);
}
