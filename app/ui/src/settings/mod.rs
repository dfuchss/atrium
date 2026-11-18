//! The settings dialog: tabs over the whole window while the dialog is open.

mod account_form;
mod accounts;
mod app_tabs;
mod browser_row;
mod custom_services;
mod service_editor;
mod service_files;

use leptos::prelude::*;
use protocol::{AppSettings, Snapshot, UiCommand};
use uuid::Uuid;

use crate::commands::{send_command, update_settings};
use crate::platform_texts::{self, Texts};
use crate::widgets::Toggle;
use accounts::AccountsTab;
use app_tabs::{AboutTab, DockTab, GeneralTab, LinksTab, NotificationsTab};
use custom_services::CustomServicesTab;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
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

    pub(crate) fn from_id(id: &str) -> Option<Tab> {
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

    fn title(self, texts: &Texts) -> &'static str {
        match self {
            Tab::Accounts => "Accounts",
            Tab::Custom => "Custom services",
            Tab::General => "General",
            Tab::Notifications => "Notifications",
            Tab::Dock => texts.badge_tab,
            Tab::Links => "Links",
            Tab::About => "About",
        }
    }
}

/// Purely local UI state: selected tab and a one-shot request to focus the add form.
#[derive(Clone, Copy)]
pub(crate) struct UiState {
    pub(crate) tab: RwSignal<Tab>,
    pub(crate) focus_add: RwSignal<bool>,
    /// Screenshot aid (`?edit=`): index of the account whose edit form opens initially (consumed once).
    pub(crate) edit_index: RwSignal<Option<usize>>,
    /// Screenshot aid (`?addcustom=1`): open the custom-service add form initially (consumed once).
    pub(crate) add_custom: RwSignal<bool>,
    /// Screenshot aid (`?editservice=<file>`): service definition file opened in the editor
    /// initially (consumed once).
    pub(crate) edit_service: RwSignal<Option<String>>,
    /// Account whose edit form opens on the accounts tab (account menu "Settings…"; consumed
    /// once).
    pub(crate) edit_account: RwSignal<Option<Uuid>>,
}

pub(crate) fn open_dialog(tab: Tab, focus_add: bool) {
    if let Some(ui) = use_context::<UiState>() {
        ui.tab.set(tab);
        ui.focus_add.set(focus_add);
    }
    send_command(UiCommand::SetDialog { open: true });
}

#[component]
pub(crate) fn SettingsDialog(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let ui = expect_context::<UiState>();
    let texts = platform_texts::texts();
    view! {
        <div class="dialog">
            <aside class="tabs">
                <h1>"Settings"</h1>
                {Tab::ALL.map(|tab| view! {
                    <button class="tab" class:active=move || ui.tab.get() == tab on:click=move |_| ui.tab.set(tab)>{tab.title(texts)}</button>
                }).into_iter().collect_view()}
                <span class="spacer"></span>
                <button class="btn" on:click=move |_| send_command(UiCommand::SetDialog { open: false })>"Close"</button>
            </aside>
            <main class="content">
                {move || match ui.tab.get() {
                    Tab::Accounts => view! { <AccountsTab snapshot=snapshot /> }.into_any(),
                    Tab::Custom => view! { <CustomServicesTab snapshot=snapshot /> }.into_any(),
                    Tab::General => view! { <GeneralTab snapshot=snapshot /> }.into_any(),
                    Tab::Notifications => view! { <NotificationsTab snapshot=snapshot /> }.into_any(),
                    Tab::Dock => view! { <DockTab snapshot=snapshot /> }.into_any(),
                    Tab::Links => view! { <LinksTab snapshot=snapshot /> }.into_any(),
                    Tab::About => view! { <AboutTab snapshot=snapshot /> }.into_any(),
                }}
            </main>
        </div>
    }
}

/// Switch row bound to one flag of the app settings.
fn app_toggle(
    snapshot: ReadSignal<Snapshot>,
    label: &'static str,
    hint: &'static str,
    get: fn(&AppSettings) -> bool,
    set: fn(&mut AppSettings, bool),
) -> impl IntoView {
    let value = Memo::new(move |_| snapshot.with(|snapshot| get(&snapshot.settings)));
    view! {
        <Toggle label=label hint=hint checked=value on_change=move |enabled| update_settings(snapshot, |settings| set(settings, enabled)) />
    }
}
