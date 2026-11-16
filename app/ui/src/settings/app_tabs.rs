//! Tabs of the app-wide settings: general, notifications, badge, links and about.

use leptos::prelude::*;
use protocol::{AfterWake, DockBadgeMode, IconSize, LinkTarget, Snapshot, Theme, UiCommand};

use super::app_toggle;
use crate::commands::{send_command, update_settings};
use crate::platform_texts;
use crate::update::UpdateSection;
use crate::widgets::{
    HIBERNATE_MINUTES, Row, after_wake_options, minutes_label, no_autocorrect, number_options,
};

#[component]
pub(super) fn GeneralTab(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let size = Memo::new(move |_| snapshot.with(|snapshot| snapshot.settings.icon_size));
    let size_button = move |value: IconSize, label: &'static str| {
        view! {
            <button class="btn" class:on=move || size.get() == value on:click=move |_| update_settings(snapshot, |settings| settings.icon_size = value)>{label}</button>
        }
    };
    let theme = Memo::new(move |_| snapshot.with(|snapshot| snapshot.settings.theme));
    let theme_button = move |value: Theme, label: &'static str| {
        view! {
            <button class="btn" class:on=move || theme.get() == value on:click=move |_| update_settings(snapshot, |settings| settings.theme = value)>{label}</button>
        }
    };
    let texts = platform_texts::texts();
    let hibernate_after = move || snapshot.with(|snapshot| snapshot.settings.hibernate_after_min);
    let after_wake = move || snapshot.with(|snapshot| snapshot.settings.after_wake);
    view! {
        <h2>"General"</h2>
        <div class="form">
            <Row label="Sidebar icon size" hint="Size of the account icons; the sidebar width follows.">
                <div class="btn-group seg">
                    {size_button(IconSize::Small, "Small")}
                    {size_button(IconSize::Medium, "Medium")}
                    {size_button(IconSize::Large, "Large")}
                </div>
            </Row>
            <Row label="Appearance" hint=texts.theme_hint>
                <div class="btn-group seg">
                    {theme_button(Theme::Auto, "Auto")}
                    {theme_button(Theme::Light, "Light")}
                    {theme_button(Theme::Dark, "Dark")}
                </div>
            </Row>
            {app_toggle(snapshot, "Launch at login", "", |settings| settings.launch_at_login, |settings, enabled| settings.launch_at_login = enabled)}
            {app_toggle(snapshot, "Start hidden", "Start without showing the window.", |settings| settings.start_hidden, |settings, enabled| settings.start_hidden = enabled)}
            {app_toggle(snapshot, "Keep running in background", "Otherwise closing the window quits the app.", |settings| settings.close_to_background, |settings, enabled| settings.close_to_background = enabled)}
            {app_toggle(snapshot, texts.tray_label, texts.tray_hint, |settings| settings.show_tray, |settings, enabled| settings.show_tray = enabled)}
            <Row label="Put inactive services to sleep after" hint="Sleeping services free memory but show no notifications or unread counts until you open them again. Selecting a service wakes it up.">
                <select prop:value=move || hibernate_after().filter(|&minutes| minutes > 0).map_or_default(|minutes| minutes.to_string())
                    on:change=move |event| { let minutes = event_target_value(&event).parse::<u32>().ok(); update_settings(snapshot, |settings| settings.hibernate_after_min = minutes) }>
                    <option value="">"Never"</option>
                    {move || number_options(&HIBERNATE_MINUTES, hibernate_after(), minutes_label)}
                </select>
            </Row>
            <Row label="After waking from sleep" hint=texts.after_wake_hint>
                <select prop:value=move || after_wake().name()
                    on:change=move |event| if let Some(mode) = AfterWake::from_name(&event_target_value(&event)) { update_settings(snapshot, |settings| settings.after_wake = mode) }>
                    {after_wake_options()}
                </select>
            </Row>
        </div>
    }
}

#[component]
pub(super) fn NotificationsTab(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    view! {
        <h2>"Notifications"</h2>
        <div class="form">
            {app_toggle(snapshot, "Do not disturb", "Suppress all notifications.", |settings| settings.dnd, |settings, enabled| settings.dnd = enabled)}
            {app_toggle(snapshot, "Message preview", "Otherwise only \u{201C}New message\u{201D} is shown.", |settings| settings.notification_preview, |settings, enabled| settings.notification_preview = enabled)}
            {app_toggle(snapshot, "Play sound", "", |settings| settings.notification_sound, |settings, enabled| settings.notification_sound = enabled)}
        </div>
    }
}

#[component]
pub(super) fn DockTab(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let mode = Memo::new(move |_| snapshot.with(|snapshot| snapshot.settings.dock_badge));
    let mode_option = move |value: DockBadgeMode, label: &'static str| {
        view! {
            <label class="radio">
                <input type="radio" name="dock-badge" prop:checked=move || mode.get() == value
                    on:change=move |_| update_settings(snapshot, |settings| settings.dock_badge = value) />
                <span>{label}</span>
            </label>
        }
    };
    view! {
        <h2>{platform_texts::texts().badge_tab}</h2>
        <div class="form">
            <Row label="Show" hint="Accounts can be excluded individually in their settings (Accounts tab).">
                {mode_option(DockBadgeMode::All, "Unread count and dot")}
                {mode_option(DockBadgeMode::CountOnly, "Unread count only")}
                {mode_option(DockBadgeMode::Off, "Off")}
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
pub(super) fn LinksTab(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let target = Memo::new(move |_| snapshot.with(|snapshot| snapshot.settings.link_target));
    let target_option = move |value: LinkTarget, label: &'static str| {
        view! {
            <label class="radio">
                <input type="radio" name="link-target" prop:checked=move || target.get() == value
                    on:change=move |_| update_settings(snapshot, |settings| settings.link_target = value) />
                <span>{label}</span>
            </label>
        }
    };
    view! {
        <h2>"Links"</h2>
        <div class="form">
            <Row label="New-window links" hint="Sign-in popups always stay in the app, so OAuth and SSO keep working.">
                {target_option(LinkTarget::SystemBrowser, "System browser")}
                {target_option(LinkTarget::AppWindow, "App window")}
            </Row>
            <BrowserRow snapshot=snapshot />
        </div>
    }
}

/// System default browser, or a named application.
#[component]
fn BrowserRow(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let browser = Memo::new(move |_| snapshot.with(|snapshot| snapshot.settings.browser.clone()));
    // Draft of the text field: committed on change/blur, never overwritten by snapshots.
    let draft = RwSignal::new(browser.get_untracked().unwrap_or_default());
    let commit = move || {
        let name = draft.get_untracked().trim().to_string();
        update_settings(snapshot, |settings| {
            settings.browser = (!name.is_empty()).then_some(name);
        });
    };
    view! {
        <Row label="Browser" hint="Enter the name of an installed application, e.g. Firefox.">
            <label class="radio">
                <input type="radio" name="browser" prop:checked=move || browser.get().is_none()
                    on:change=move |_| update_settings(snapshot, |settings| settings.browser = None) />
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
                    on:input=move |event| draft.set(event_target_value(&event))
                    on:change=move |_| commit()
                    use:no_autocorrect />
                <datalist id="browsers">
                    {BROWSERS.map(|name| view! { <option value=name></option> }).into_iter().collect_view()}
                </datalist>
            </label>
        </Row>
    }
}

#[component]
pub(super) fn AboutTab(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let app = Memo::new(move |_| snapshot.with(|snapshot| snapshot.app.clone()));
    view! {
        <h2>"About"</h2>
        <div class="about-head">
            <img class="about-logo" src="atrium.svg" alt="" />
            <div>
                <h3 class="about-name">"Atrium"</h3>
                <div class="about-version">{move || format!("Version {}", app.get().version)}</div>
                <div class="about-tagline">"All your messengers in one calm place."</div>
            </div>
        </div>
        <p class="note">"Atrium is a lightweight messenger aggregator for macOS and Linux. Every service account runs in its own isolated webview with its own data store, so logins and cookies stay separate."</p>
        <p class="note">"\u{a9} 2026 Dominik Fuchß \u{b7} GNU GPL v3 or later"</p>
        <div class="form">
            <Row label="Project page"><code>{protocol::PROJECT_URL}</code></Row>
            <Row label="">
                <button class="btn" on:click=move |_| send_command(UiCommand::OpenProjectPage)>"Open project page"</button>
            </Row>
            <Row label="Config directory"><code>{move || app.get().config_dir}</code></Row>
            <Row label="Log file"><code>{move || app.get().log_file}</code></Row>
            <Row label="Services directory"><code>{move || app.get().services_dir}</code></Row>
            {platform_texts::texts().web_process_sandbox.map(|state| view! {
                <Row label="Web process sandbox">{state}</Row>
            })}
        </div>
        <UpdateSection />
    }
}
