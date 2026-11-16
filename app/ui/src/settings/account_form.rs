//! Inline edit form of one account.

use leptos::prelude::*;
use protocol::{AccountSettings, AccountView, AfterWake, LinkTarget, UiCommand};
use uuid::Uuid;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

use crate::commands::send_command;
use crate::platform_texts;
use crate::widgets::{
    HIBERNATE_MINUTES, Row, Toggle, after_wake_label, after_wake_options, hibernate_label,
    instance_field_label, minutes_label, no_autocorrect, non_empty, number_options,
};

/// Uploaded icons larger than this are rejected (they are stored in `accounts.json`).
const MAX_ICON_BYTES: f64 = 256.0 * 1024.0;
/// Zoom levels offered, in percent.
const ZOOM_PERCENTS: [u32; 10] = [50, 67, 80, 90, 100, 110, 125, 150, 175, 200];

/// Splits a stored user agent into (select mode, custom text).
pub(super) fn user_agent_mode(
    user_agent: Option<&str>,
    unset_mode: &'static str,
) -> (String, String) {
    match user_agent {
        None => (unset_mode.to_string(), String::new()),
        Some(preset @ ("webkit" | "chrome" | "default")) => (preset.to_string(), String::new()),
        Some(literal) => ("custom".to_string(), literal.to_string()),
    }
}

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
    error: RwSignal<String>,
) {
    error.set(String::new());
    let Some(file) = input.files().and_then(|files| files.get(0)) else {
        return;
    };
    if file.size() > MAX_ICON_BYTES {
        error.set(format!(
            "Image is too large ({} KB); the limit is 256 KB.",
            (file.size() / 1024.0).ceil()
        ));
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
        error.set("Could not read the image.".into());
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
    in_dock_badge: RwSignal<bool>,
    enabled: RwSignal<bool>,
    start_asleep: RwSignal<bool>,
    /// "" (global setting), "system" or "app".
    link_target: RwSignal<&'static str>,
    custom_icon: RwSignal<Option<String>>,
    icon_url: RwSignal<String>,
    icon_error: RwSignal<String>,
    simple_icon: RwSignal<String>,
    zoom_percent: RwSignal<String>,
    hibernate_after: RwSignal<String>,
    /// "" (global setting) or an `AfterWake` name.
    after_wake: RwSignal<String>,
    custom_css: RwSignal<String>,
    app_hosts: RwSignal<String>,
    /// "" (service default), "allow" or "block".
    allow_media: RwSignal<&'static str>,
    user_agent_mode: RwSignal<String>,
    user_agent_custom: RwSignal<String>,
}

impl AccountDraft {
    fn new(settings: &AccountSettings) -> AccountDraft {
        let (mode, custom) = user_agent_mode(settings.user_agent.as_deref(), "");
        AccountDraft {
            label: RwSignal::new(settings.label.clone()),
            instance: RwSignal::new(settings.instance.clone().unwrap_or_default()),
            notifications: RwSignal::new(!settings.muted),
            show_badge: RwSignal::new(settings.show_badge),
            in_dock_badge: RwSignal::new(settings.in_dock_badge),
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
            icon_error: RwSignal::new(String::new()),
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
            user_agent_mode: RwSignal::new(mode),
            user_agent_custom: RwSignal::new(custom),
        }
    }

    fn user_agent(self) -> Option<String> {
        match self.user_agent_mode.get_untracked().as_str() {
            "" => None,
            "custom" => non_empty(&self.user_agent_custom.get_untracked()),
            preset => Some(preset.to_string()),
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
            in_dock_badge: self.in_dock_badge.get_untracked(),
            user_agent: self.user_agent(),
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
                <input type="text" spellcheck="false" prop:value=move || draft.label.get() on:input=move |event| draft.label.set(event_target_value(&event)) />
            </Row>
            {instance_label.map(|instance_label| view! {
                <Row label=instance_field_label(instance_label) hint="Changing the instance reloads the account.">
                    <input type="text" autocapitalize="off" spellcheck="false" autocomplete="off"
                        prop:value=move || draft.instance.get() on:input=move |event| draft.instance.set(event_target_value(&event)) use:no_autocorrect />
                </Row>
            })}
            <Toggle label="Enabled" hint="Disabled accounts keep their login but are not loaded." checked=draft.enabled on_change=move |enabled| draft.enabled.set(enabled) />
            <Toggle label="Notifications" checked=draft.notifications on_change=move |enabled| draft.notifications.set(enabled) />
            <Toggle label="Unread badge" checked=draft.show_badge on_change=move |enabled| draft.show_badge.set(enabled) />
            <Toggle label=platform_texts::texts().account_badge_toggle checked=draft.in_dock_badge on_change=move |enabled| draft.in_dock_badge.set(enabled) />
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
            <Row label="">
                <div class="actions">
                    <button class="btn primary" type="submit">"Save"</button>
                    <button class="btn" type="button" on:click=move |_| editing.set(None)>"Cancel"</button>
                </div>
            </Row>
        </form>
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
        match draft.custom_icon.get() {
        Some(source) => view! { <span class="icon small has-img"><img src=source alt="" /></span> }.into_any(),
        None => match glyph_icon.clone().filter(|_| !draft.simple_icon.get().trim().is_empty()).or_else(|| site_icon.clone()) {
            Some(source) if site_icon.as_ref() != Some(&source) => view! { <span class="icon small has-img glyph" style=format!("background:{color}")><img src=source alt="" /></span> }.into_any(),
            Some(source) => view! { <span class="icon small has-img"><img src=source alt="" /></span> }.into_any(),
            None => view! { <span class="icon small" style=format!("background:{color}")>{initials.clone()}</span> }.into_any(),
        },
    }
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
        draft.icon_error.set(String::new());
    };
    let apply_icon_url = move |_| {
        let url = draft.icon_url.get_untracked().trim().to_string();
        draft.icon_error.set(String::new());
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
            {move || { let error = draft.icon_error.get(); (!error.is_empty()).then(|| view! { <span class="error">{error}</span> }) }}
        </Row>
        <Row label="Simple Icons" hint="Brand glyph on the service color (slug from simpleicons.org, letters and digits only). Precedence: custom image, Simple Icon, site icon, initials.">
            <input type="text" class="wide" placeholder="e.g. discord"
                autocapitalize="off" spellcheck="false" autocomplete="off"
                prop:value=move || draft.simple_icon.get() on:input=move |event| draft.simple_icon.set(event_target_value(&event)) use:no_autocorrect />
        </Row>
        <Row label="Image URL">
            <input type="url" class="wide" placeholder="https://example.com/icon.png"
                autocapitalize="off" spellcheck="false" autocomplete="off"
                prop:value=move || draft.icon_url.get()
                on:input=move |event| draft.icon_url.set(event_target_value(&event))
                on:change=apply_icon_url use:no_autocorrect />
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
        <Row label="User agent" hint="Changing the user agent reloads the account.">
            <select prop:value=move || draft.user_agent_mode.get() on:change=move |event| draft.user_agent_mode.set(event_target_value(&event))>
                <option value="">"Service default"</option>
                <option value="webkit">"WebKit browser"</option>
                <option value="chrome">"Chrome"</option>
                <option value="default">"System WebKit"</option>
                <option value="custom">"Custom\u{2026}"</option>
            </select>
        </Row>
        {move || (draft.user_agent_mode.get() == "custom").then(|| view! {
            <Row label="Custom user agent">
                <input type="text" class="wide" autocapitalize="off" spellcheck="false" autocomplete="off"
                    prop:value=move || draft.user_agent_custom.get() on:input=move |event| draft.user_agent_custom.set(event_target_value(&event)) use:no_autocorrect />
            </Row>
        })}
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
