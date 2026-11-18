//! Inline edit form of one account.

use leptos::prelude::*;
use protocol::{
    AccountSettings, AccountView, AfterWake, BadgeStyle, DockBadgeMode, LinkTarget, UiCommand,
};
use uuid::Uuid;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

use super::user_agent::{UserAgentDraft, UserAgentRows};
use crate::commands::send_command;
use crate::platform_texts;
use crate::widgets::{
    ErrorNote, FormActions, HIBERNATE_MINUTES, IconFace, Row, TextField, Toggle, after_wake_label,
    after_wake_options, badge_style_label, dock_badge_label, hibernate_label, icon_view,
    instance_field_label, minutes_label, no_autocorrect, non_empty, number_options,
};

/// Uploaded icons larger than this are rejected (they are stored in `accounts.json`).
const MAX_ICON_BYTES: f64 = 256.0 * 1024.0;
const ZOOM_PERCENTS: [u32; 10] = [50, 67, 80, 90, 100, 110, 125, 150, 175, 200];

/// User agent mode of an account that uses its service's user agent.
const UNSET_USER_AGENT: &str = "";

/// Zoom factor as whole percent, at least 1.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded and clamped to at least 1 before the saturating cast"
)]
fn zoom_percent(zoom: f64) -> u32 {
    (zoom * 100.0).round().max(1.0) as u32
}

/// Reads the chosen image file as a data URL into `icon`; files over 256 KB are rejected.
fn read_image(
    input: &web_sys::HtmlInputElement,
    icon: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
) {
    error.set(None);
    let Some(file) = input.files().and_then(|files| files.get(0)) else {
        return;
    };
    if file.size() > MAX_ICON_BYTES {
        error.set(Some(format!(
            "Image is too large ({} KB); the limit is 256 KB.",
            (file.size() / 1024.0).ceil()
        )));
        input.set_value("");
        return;
    }
    let Ok(reader) = web_sys::FileReader::new() else {
        return;
    };
    let loaded_reader = reader.clone();
    let on_load = Closure::once_into_js(move || {
        if let Ok(result) = loaded_reader.result()
            && let Some(data_url) = result.as_string()
        {
            icon.set(Some(data_url));
        }
    });
    reader.set_onload(Some(on_load.unchecked_ref()));
    if reader.read_as_data_url(&file).is_err() {
        error.set(Some("Could not read the image.".into()));
    }
    input.set_value("");
}

/// Editable copy of an account's settings, one signal per field.
#[derive(Clone, Copy)]
struct AccountDraft {
    label: RwSignal<String>,
    instance: RwSignal<String>,
    notifications: RwSignal<bool>,
    show_badge: RwSignal<bool>,
    /// `None`: the global setting.
    badge_style: RwSignal<Option<BadgeStyle>>,
    /// `None`: the global setting.
    dock_badge: RwSignal<Option<DockBadgeMode>>,
    enabled: RwSignal<bool>,
    start_asleep: RwSignal<bool>,
    /// "" (global setting), "system" or "app".
    link_target: RwSignal<&'static str>,
    custom_icon: RwSignal<Option<String>>,
    icon_url: RwSignal<String>,
    icon_error: RwSignal<Option<String>>,
    simple_icon: RwSignal<String>,
    zoom_percent: RwSignal<String>,
    hibernate_after: RwSignal<String>,
    /// "" (global setting) or an `AfterWake` name.
    after_wake: RwSignal<String>,
    custom_css: RwSignal<String>,
    app_hosts: RwSignal<String>,
    /// "" (service default), "allow" or "block".
    allow_media: RwSignal<&'static str>,
    user_agent: UserAgentDraft,
}

impl AccountDraft {
    fn new(settings: &AccountSettings) -> AccountDraft {
        AccountDraft {
            label: RwSignal::new(settings.label.clone()),
            instance: RwSignal::new(settings.instance.clone().unwrap_or_default()),
            notifications: RwSignal::new(!settings.muted),
            show_badge: RwSignal::new(settings.show_badge),
            badge_style: RwSignal::new(settings.badge_style),
            dock_badge: RwSignal::new(settings.dock_badge),
            enabled: RwSignal::new(settings.enabled),
            start_asleep: RwSignal::new(settings.start_asleep),
            link_target: RwSignal::new(match settings.link_target {
                None => "",
                Some(LinkTarget::SystemBrowser) => "system",
                Some(LinkTarget::AppWindow) => "app",
            }),
            custom_icon: RwSignal::new(settings.custom_icon.clone()),
            icon_url: RwSignal::new(
                settings
                    .custom_icon
                    .clone()
                    .filter(|icon| !icon.starts_with("data:"))
                    .unwrap_or_default(),
            ),
            icon_error: RwSignal::new(None),
            simple_icon: RwSignal::new(settings.simple_icon.clone().unwrap_or_default()),
            zoom_percent: RwSignal::new(zoom_percent(settings.zoom).to_string()),
            hibernate_after: RwSignal::new(
                settings
                    .hibernate_after_min
                    .map_or_default(|minutes| minutes.to_string()),
            ),
            after_wake: RwSignal::new(
                settings
                    .after_wake
                    .map_or_default(|mode| mode.name().to_string()),
            ),
            custom_css: RwSignal::new(settings.custom_css.clone().unwrap_or_default()),
            app_hosts: RwSignal::new(settings.app_hosts.join("\n")),
            allow_media: RwSignal::new(match settings.allow_media {
                None => "",
                Some(true) => "allow",
                Some(false) => "block",
            }),
            user_agent: UserAgentDraft::new(settings.user_agent.as_deref(), UNSET_USER_AGENT),
        }
    }

    /// The settings to send; an empty label falls back to `service_name`.
    fn to_settings(self, service_name: &str, has_instance: bool) -> AccountSettings {
        let custom_css = self.custom_css.get_untracked();
        AccountSettings {
            label: non_empty(&self.label.get_untracked())
                .unwrap_or_else(|| service_name.to_string()),
            instance: non_empty(&self.instance.get_untracked()).filter(|_| has_instance),
            muted: !self.notifications.get_untracked(),
            show_badge: self.show_badge.get_untracked(),
            badge_style: self.badge_style.get_untracked(),
            dock_badge: self.dock_badge.get_untracked(),
            user_agent: self.user_agent.value(UNSET_USER_AGENT),
            enabled: self.enabled.get_untracked(),
            link_target: match self.link_target.get_untracked() {
                "system" => Some(LinkTarget::SystemBrowser),
                "app" => Some(LinkTarget::AppWindow),
                _ => None,
            },
            custom_icon: self
                .custom_icon
                .get_untracked()
                .filter(|icon| !icon.is_empty()),
            simple_icon: non_empty(&self.simple_icon.get_untracked()),
            zoom: self
                .zoom_percent
                .get_untracked()
                .parse::<f64>()
                .map_or(1.0, |percent| percent / 100.0),
            hibernate_after_min: self.hibernate_after.get_untracked().parse().ok(),
            start_asleep: self.start_asleep.get_untracked(),
            after_wake: AfterWake::from_name(&self.after_wake.get_untracked()),
            custom_css: (!custom_css.trim().is_empty()).then_some(custom_css),
            allow_media: match self.allow_media.get_untracked() {
                "allow" => Some(true),
                "block" => Some(false),
                _ => None,
            },
            app_hosts: self
                .app_hosts
                .get_untracked()
                .lines()
                .filter_map(non_empty)
                .collect(),
        }
    }
}

/// Inline edit form. The draft is copied from the account once, when the form opens; later
/// snapshots do not touch it.
#[component]
pub(super) fn EditForm(
    account: AccountView,
    editing: RwSignal<Option<Uuid>>,
    media_default: bool,
    global_hibernate: Option<u32>,
    global_after_wake: AfterWake,
    global_badge_style: BadgeStyle,
    global_dock_badge: DockBadgeMode,
) -> impl IntoView {
    let id = account.id;
    let loaded = !account.hibernated;
    let draft = AccountDraft::new(&account.settings);
    let service_name = account.service_name.clone();
    let instance_label = account.instance_label.clone();
    let has_instance = instance_label.is_some();
    let save = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        send_command(UiCommand::UpdateAccount {
            account: id,
            settings: draft.to_settings(&service_name, has_instance),
        });
        editing.set(None);
    };
    view! {
        <form class="edit form" novalidate on:submit=save>
            <Row label="Label">
                <TextField value=draft.label plain=true />
            </Row>
            {instance_label.map(|instance_label| view! {
                <Row label=instance_field_label(instance_label) hint="Changing the instance reloads the account.">
                    <TextField value=draft.instance />
                </Row>
            })}
            <Toggle label="Enabled" hint="Disabled accounts keep their login but are not loaded." checked=draft.enabled on_change=move |enabled| draft.enabled.set(enabled) />
            <Toggle label="Notifications" checked=draft.notifications on_change=move |enabled| draft.notifications.set(enabled) />
            <Toggle label="Unread badge" checked=draft.show_badge on_change=move |enabled| draft.show_badge.set(enabled) />
            <Row label="Badge style">
                <div class="btn-group seg">
                    {draft_segment(draft.badge_style, None, "Global")}
                    {BadgeStyle::ALL.map(|style| draft_segment(draft.badge_style, Some(style), badge_style_label(style)))}
                </div>
                <span class="hint">{format!("Global setting: {}.", badge_style_label(global_badge_style))}</span>
            </Row>
            <Row label=platform_texts::texts().badge_tab>
                <div class="btn-group seg">
                    {draft_segment(draft.dock_badge, None, "Global")}
                    {DockBadgeMode::ALL.map(|mode| draft_segment(draft.dock_badge, Some(mode), dock_badge_label(mode)))}
                </div>
                <span class="hint">{format!("Global setting: {}.", dock_badge_label(global_dock_badge))}</span>
            </Row>
            <Row label="Open links in">
                <select prop:value=move || draft.link_target.get() on:change=move |event| draft.link_target.set(match event_target_value(&event).as_str() {
                    "system" => "system",
                    "app" => "app",
                    _ => "",
                })>
                    <option value="">"Use global setting"</option>
                    <option value="system">"System browser"</option>
                    <option value="app">"App window"</option>
                </select>
            </Row>
            <IconRows account=account draft=draft />
            <BehaviourRows draft=draft media_default=media_default global_hibernate=global_hibernate global_after_wake=global_after_wake account=id loaded=loaded />
            <FormActions disabled=false on_cancel=move |()| editing.set(None) />
        </form>
    }
}

/// Button of a segmented control bound to a draft field; `value` `None` is the global setting.
fn draft_segment<T: Copy + PartialEq + Send + Sync + 'static>(
    field: RwSignal<Option<T>>,
    value: Option<T>,
    label: &'static str,
) -> impl IntoView {
    view! {
        <button type="button" class="btn" class:on=move || field.get() == value on:click=move |_| field.set(value)>{label}</button>
    }
}

/// Preview of the icon the account will show with the draft's choices.
fn icon_preview(account: &AccountView, draft: &AccountDraft) -> impl Fn() -> AnyView + use<> {
    let draft = *draft;
    let uses_custom = account.settings.custom_icon.is_some();
    let site_icon = account
        .icon
        .clone()
        .filter(|_| !uses_custom && !account.icon_glyph);
    let glyph_icon = account
        .icon
        .clone()
        .filter(|_| !uses_custom && account.icon_glyph);
    let (color, initials) = (account.color.clone(), account.initials.clone());
    move || {
        let face = match draft.custom_icon.get() {
            Some(source) => IconFace::Image(source),
            None => glyph_icon
                .clone()
                .filter(|_| !draft.simple_icon.get().trim().is_empty())
                .map(IconFace::Glyph)
                .or_else(|| site_icon.clone().map(IconFace::Image))
                .unwrap_or_else(|| IconFace::Initials(initials.clone())),
        };
        icon_view(face, &color, true)
    }
}

/// Custom image, Simple Icons slug and image URL.
#[component]
fn IconRows(account: AccountView, draft: AccountDraft) -> impl IntoView {
    let id = account.id;
    let use_site_icon = move |_| {
        draft.custom_icon.set(None);
        draft.simple_icon.set(String::new());
        draft.icon_url.set(String::new());
        draft.icon_error.set(None);
    };
    let apply_icon_url = move |()| {
        let url = draft.icon_url.get_untracked().trim().to_string();
        draft.icon_error.set(None);
        if !url.is_empty() {
            draft.custom_icon.set(Some(url));
        } else if draft
            .custom_icon
            .get_untracked()
            .is_some_and(|icon| !icon.starts_with("data:"))
        {
            draft.custom_icon.set(None);
        }
    };
    view! {
        <Row label="Icon">
            <div class="icon-row">
                {icon_preview(&account, &draft)}
                <div class="icon-actions">
                    <label class="btn small file-btn">
                        "Choose image\u{2026}"
                        <input type="file" accept="image/*" hidden
                            on:change=move |event| {
                                draft.icon_url.set(String::new());
                                read_image(&event_target::<web_sys::HtmlInputElement>(&event), draft.custom_icon, draft.icon_error);
                            } />
                    </label>
                    <button class="btn small" type="button" on:click=use_site_icon>"Use site icon"</button>
                    <button class="btn small" type="button" on:click=move |_| send_command(UiCommand::RefreshIcon { account: id })>"Refresh site icon"</button>
                </div>
            </div>
            <ErrorNote message=draft.icon_error />
        </Row>
        <Row label="Simple Icons" hint="Brand glyph on the service color (slug from simpleicons.org, letters and digits only). Precedence: custom image, Simple Icon, site icon, initials.">
            <TextField value=draft.simple_icon class="wide" placeholder="e.g. discord" />
        </Row>
        <Row label="Image URL">
            <TextField value=draft.icon_url kind="url" class="wide" placeholder="https://example.com/icon.png" on_change=apply_icon_url />
        </Row>
    }
}

/// Media permission, zoom, sleep, user agent, app hosts and custom CSS.
#[component]
fn BehaviourRows(
    draft: AccountDraft,
    media_default: bool,
    global_hibernate: Option<u32>,
    global_after_wake: AfterWake,
    account: Uuid,
    /// The account has a view to reload (neither sleeping nor disabled).
    loaded: bool,
) -> impl IntoView {
    let zoom_options = number_options(
        &ZOOM_PERCENTS,
        draft.zoom_percent.get_untracked().parse().ok(),
        |percent| format!("{percent} %"),
    );
    let hibernate_options = number_options(
        &HIBERNATE_MINUTES,
        draft.hibernate_after.get_untracked().parse().ok(),
        minutes_label,
    );
    view! {
        <Row label="Camera & microphone">
            <select prop:value=move || draft.allow_media.get() on:change=move |event| draft.allow_media.set(match event_target_value(&event).as_str() {
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
            <select prop:value=move || draft.zoom_percent.get() on:change=move |event| draft.zoom_percent.set(event_target_value(&event))>{zoom_options}</select>
        </Row>
        <Row label="Sleep in background">
            <select prop:value=move || draft.hibernate_after.get() on:change=move |event| draft.hibernate_after.set(event_target_value(&event))>
                <option value="">{format!("Use global setting ({})", hibernate_label(global_hibernate))}</option>
                <option value="0">"Never"</option>
                {hibernate_options}
            </select>
        </Row>
        <Toggle label="Start asleep" hint="Don't load this service when Atrium starts; it wakes when you select it. Takes effect at the next start." checked=draft.start_asleep on_change=move |enabled| draft.start_asleep.set(enabled) />
        <Row label="After waking from sleep" hint=platform_texts::texts().after_wake_hint>
            <select prop:value=move || draft.after_wake.get() on:change=move |event| draft.after_wake.set(event_target_value(&event))>
                <option value="">{format!("Use global setting ({})", after_wake_label(global_after_wake))}</option>
                {after_wake_options()}
            </select>
        </Row>
        <Row label="Force reload" hint="Reloads the page without the cache, for a service that stays broken after a normal reload. Sleeping and disabled services have nothing to reload.">
            <button class="btn small" type="button" disabled=!loaded on:click=move |_| send_command(UiCommand::ForceReload { account })>"Force reload"</button>
        </Row>
        <UserAgentRows draft=draft.user_agent unset_mode=UNSET_USER_AGENT hint="Changing the user agent reloads the account." />
        <Row label="Always open in the app" hint="Links that open a new window to these hosts open inside the app with this account's login (e.g. a single-sign-on provider such as idp.example.org, or *.microsoftonline.com). Redirects within the page always stay in the app.">
            <textarea class="mono" rows="3" autocapitalize="off" spellcheck="false" autocomplete="off" placeholder="one host per line"
                prop:value=move || draft.app_hosts.get() on:input=move |event| draft.app_hosts.set(event_target_value(&event)) use:no_autocorrect ></textarea>
        </Row>
        <Row label="Custom CSS" hint="Injected into every page; changing it reloads the account.">
            <textarea class="mono" rows="6" autocapitalize="off" spellcheck="false" autocomplete="off" placeholder="/* injected into every page */"
                prop:value=move || draft.custom_css.get() on:input=move |event| draft.custom_css.set(event_target_value(&event)) use:no_autocorrect ></textarea>
        </Row>
    }
}
