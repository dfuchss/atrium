//! Tabs of the app-wide settings: general, notifications, dock badge, links and about.

use leptos::prelude::*;
use protocol::{DockBadgeMode, IconSize, LinkTarget, Snapshot, UiCommand};

use super::app_toggle;
use crate::commands::{send_command, update_settings};
use crate::widgets::{HIBERNATE_MINUTES, Row, minutes_label, no_autocorrect, number_options};

#[component]
pub(super) fn GeneralTab(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let size = Memo::new(move |_| snapshot.with(|snapshot| snapshot.settings.icon_size));
    let size_button = move |value: IconSize, label: &'static str| {
        view! {
            <button class="btn" class:on=move || size.get() == value on:click=move |_| update_settings(snapshot, |settings| settings.icon_size = value)>{label}</button>
        }
    };
    let hibernate_after = move || snapshot.with(|snapshot| snapshot.settings.hibernate_after_min);
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
            {app_toggle(snapshot, "Launch at login", "", |settings| settings.launch_at_login, |settings, enabled| settings.launch_at_login = enabled)}
            {app_toggle(snapshot, "Start hidden", "Start without showing the window.", |settings| settings.start_hidden, |settings, enabled| settings.start_hidden = enabled)}
            {app_toggle(snapshot, "Keep running in background", "Otherwise closing the window quits the app.", |settings| settings.close_to_background, |settings, enabled| settings.close_to_background = enabled)}
            {app_toggle(snapshot, "Menu bar icon", "Unread count and account menu in the menu bar.", |settings| settings.show_tray, |settings, enabled| settings.show_tray = enabled)}
            <Row label="Put inactive services to sleep after" hint="Sleeping services free memory but show no notifications or unread counts until you open them again. Selecting a service wakes it up.">
                <select prop:value=move || hibernate_after().filter(|&minutes| minutes > 0).map(|minutes| minutes.to_string()).unwrap_or_default()
                    on:change=move |event| { let minutes = event_target_value(&event).parse::<u32>().ok(); update_settings(snapshot, |settings| settings.hibernate_after_min = minutes) }>
                    <option value="">"Never"</option>
                    {move || number_options(&HIBERNATE_MINUTES, hibernate_after(), minutes_label)}
                </select>
            </Row>
            {app_toggle(snapshot, "Reload after wake", "Reload services after waking from sleep; fixes stale connections.", |settings| settings.reload_after_wake, |settings, enabled| settings.reload_after_wake = enabled)}
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
        <h2>"Dock badge"</h2>
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
            settings.browser = (!name.is_empty()).then_some(name)
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

/// The Atrium logo (see `app/src-tauri/icons/src/atrium.svg`), inline so it needs no asset.
const LOGO_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="100 100 824 824" width="96" height="96" aria-hidden="true"><defs><linearGradient id="ab-bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#0f766e"/><stop offset="1" stop-color="#312e81"/></linearGradient><radialGradient id="ab-light" cx="0.5" cy="0.4" r="0.7"><stop offset="0" stop-color="#fffbeb"/><stop offset="0.45" stop-color="#fde68a"/><stop offset="1" stop-color="#f59e0b"/></radialGradient><clipPath id="ab-roof"><rect x="232" y="232" width="560" height="560" rx="70"/></clipPath></defs><rect x="100" y="100" width="824" height="824" rx="185" fill="url(#ab-bg)"/><g clip-path="url(#ab-roof)"><polygon points="232,232 792,232 642,382 382,382" fill="#fff" fill-opacity="0.97"/><polygon points="792,232 792,792 642,642 642,382" fill="#fff" fill-opacity="0.72"/><polygon points="232,792 792,792 642,642 382,642" fill="#fff" fill-opacity="0.55"/><polygon points="232,232 232,792 382,642 382,382" fill="#fff" fill-opacity="0.84"/></g><rect x="382" y="382" width="260" height="260" rx="22" fill="url(#ab-light)"/><rect x="382" y="382" width="260" height="260" rx="22" fill="none" stroke="#fff" stroke-opacity="0.5" stroke-width="6"/></svg>"##;

#[component]
pub(super) fn AboutTab(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let app = Memo::new(move |_| snapshot.with(|snapshot| snapshot.app.clone()));
    view! {
        <h2>"About"</h2>
        <div class="about-head">
            <span class="about-logo" inner_html=LOGO_SVG></span>
            <div>
                <h3 class="about-name">"Atrium"</h3>
                <div class="about-version">{move || format!("Version {}", app.get().version)}</div>
                <div class="about-tagline">"All your messengers in one calm place."</div>
            </div>
        </div>
        <p class="note">"Atrium is a lightweight messenger aggregator for macOS. Every service account runs in its own isolated webview with its own data store, so logins and cookies stay separate."</p>
        <p class="note">"\u{a9} 2026 Dominik Fuchß \u{b7} GNU GPL v3 or later"</p>
        <div class="form">
            <Row label="Project page"><code>{protocol::PROJECT_URL}</code></Row>
            <Row label="">
                <button class="btn" on:click=move |_| send_command(UiCommand::OpenProjectPage)>"Open project page"</button>
            </Row>
            <Row label="Config directory"><code>{move || app.get().config_dir}</code></Row>
            <Row label="Log file"><code>{move || app.get().log_file}</code></Row>
            <Row label="Services directory"><code>{move || app.get().services_dir}</code></Row>
        </div>
    }
}
