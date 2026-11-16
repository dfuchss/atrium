//! The reducer: `Core::handle(Input) -> Vec<Effect>`.
//!
//! `Core` owns the persisted model and the runtime state of the views. This module routes
//! inputs; each submodule handles one feature.

mod accounts;
mod icons;
mod layout;
mod menus;
mod messages;
mod scripts;
mod services;
mod settings;
mod snapshot;
mod tick;
mod unread;
mod views;
mod wake;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use protocol::{AppInfo, AppSettings, UiCommand};
use uuid::Uuid;

use crate::catalog::{Catalog, ServiceDef};
use crate::model::{Account, Persisted};
use crate::throttle::RateLimiter;

pub use icons::{normalize_slug, simple_icon_url};
pub use messages::{
    Bounds, DockBadge, Effect, Input, MenuAccount, MenuAction, MenuModel, NotificationReq,
    SimpleIconResult, ViewPolicy, ViewSpec,
};
pub use scripts::ICON_SCRIPT;
pub use settings::ZOOM_STEPS;
pub use tick::SLEEP_GAP_MS;
pub use unread::HIDDEN_PREVIEW_BODY;

use layout::WindowSize;
use unread::{TitleRules, UnreadTracker};
use wake::{CRASH_RELOAD_WINDOW_MS, MAX_CRASH_RELOADS};

pub struct Core {
    /// Built-in + user TOML services; `catalog` = this + custom services.
    base_catalog: Catalog,
    /// The user's TOML definitions applied on top of `builtin_catalog` in `base_catalog`.
    user_service_defs: Vec<ServiceDef>,
    /// Problems of the last load of the user's service files.
    service_errors: Vec<String>,
    /// The catalog without user files (built-ins only), the base of every reload.
    builtin_catalog: Catalog,
    catalog: Catalog,
    /// Unread title patterns per service id, compiled from `catalog`.
    title_rules: HashMap<String, TitleRules>,
    persisted: Persisted,
    new_account_id: Box<dyn FnMut() -> Uuid + Send>,
    /// The host's part of the user agent presets, e.g. `X11; Linux x86_64`.
    user_agent_platform: String,
    active: Option<Uuid>,
    dialog_open: bool,
    window: WindowSize,
    started: bool,
    unread: UnreadTracker,
    last_badge: Option<DockBadge>,
    last_menus: Option<MenuModel>,
    app_info: AppInfo,
    /// Accounts whose view currently exists.
    loaded_views: HashSet<Uuid>,
    /// Last time (tick clock) each account was active (shown) or left.
    last_activity_ms: HashMap<Uuid, u64>,
    /// Latest wall clock reading (ticks and resumes).
    last_tick_ms: Option<u64>,
    /// A sleep was detected and not yet handled; the next resume does it.
    slept: bool,
    /// Accounts checked after a wake (`AfterWake::Check`) that have not answered a poll yet,
    /// with the time of the wake (tick clock).
    wake_checks: HashMap<Uuid, u64>,
    /// Favicon lookups started this run, per account (retried on later page loads after a failure).
    favicon_attempts: HashMap<Uuid, u8>,
    /// Problems found while loading (logged on start).
    startup_log: Vec<String>,
    /// Write page titles into the log (diagnosing unread patterns); off by default.
    log_page_titles: bool,
    /// External opens (browser, mail app) per account, so a page cannot flood the desktop.
    external_link_limit: RateLimiter<Uuid>,
    /// Reloads after a crashed web process per account, so a crashing page is not reloaded
    /// forever.
    crash_reload_limit: RateLimiter<Uuid>,
    /// Accounts whose crashes are no longer reloaded (logged once).
    crash_reloads_stopped: HashSet<Uuid>,
}

/// One external open per account within this time (tick clock).
const EXTERNAL_LINK_INTERVAL_MS: u64 = 2_000;

impl Core {
    /// `new_account_id` creates ids for new accounts (`Uuid::new_v4` in the app, deterministic
    /// in tests). `catalog` holds the built-in and user TOML services; the persisted custom
    /// services are added on top. `user_agent_platform` is the host's part of the user agent
    /// presets (see [`crate::catalog::webkit_user_agent`]).
    pub fn new(
        catalog: Catalog,
        persisted: Persisted,
        new_account_id: Box<dyn FnMut() -> Uuid + Send>,
        user_agent_platform: String,
    ) -> Core {
        let active = persisted.settings.active;
        let mut core = Core {
            catalog: catalog.clone(),
            builtin_catalog: catalog.clone(),
            base_catalog: catalog,
            user_service_defs: vec![],
            service_errors: vec![],
            title_rules: HashMap::new(),
            persisted,
            new_account_id,
            user_agent_platform,
            active,
            dialog_open: false,
            window: WindowSize::default(),
            started: false,
            unread: UnreadTracker::default(),
            last_badge: None,
            last_menus: None,
            app_info: AppInfo::default(),
            loaded_views: HashSet::new(),
            last_activity_ms: HashMap::new(),
            last_tick_ms: None,
            slept: false,
            wake_checks: HashMap::new(),
            favicon_attempts: HashMap::new(),
            startup_log: vec![],
            log_page_titles: false,
            external_link_limit: RateLimiter::new(1, EXTERNAL_LINK_INTERVAL_MS),
            crash_reload_limit: RateLimiter::new(MAX_CRASH_RELOADS, CRASH_RELOAD_WINDOW_MS),
            crash_reloads_stopped: HashSet::new(),
        };
        core.startup_log = core.rebuild_catalog();
        core
    }

    /// Whether title changes are logged with the title itself (it names chats and senders).
    pub fn set_log_page_titles(&mut self, enabled: bool) {
        self.log_page_titles = enabled;
    }

    /// Read-only app information shown in the snapshot ("About").
    pub fn set_app_info(&mut self, info: AppInfo) {
        self.app_info = info;
    }

    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        let mut effects = self.dispatch(input);
        effects.extend(self.menus_if_changed());
        effects
    }

    fn dispatch(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Started { width, height } => self.started(WindowSize { width, height }),
            Input::Ui(command) => self.ui(command),
            Input::Menu(action) => self.menu(action),
            Input::TitleChanged { account, title } => self.title_changed(account, &title),
            Input::PollResult { account, raw } => {
                let alive = self.wake_check_answered(account, &raw);
                alive
                    .into_iter()
                    .chain(self.poll_result(account, &raw))
                    .collect()
            }
            Input::PageLoaded { account, .. } => self.page_loaded(account),
            Input::ExternalLink { account, url } => self.open_external_link(account, url),
            Input::NotificationClicked { account, shim_id } => {
                self.notification_clicked(account, shim_id)
            }
            Input::DownloadFinished { account, file_name } => {
                self.download_finished(account, &file_name)
            }
            Input::WindowResized { width, height } => {
                self.window_resized(WindowSize { width, height })
            }
            Input::Tick { now_ms } => self.tick(now_ms),
            Input::Resumed { now_ms } => self.resumed(now_ms),
            Input::WebProcessTerminated { account } => self.web_process_terminated(account),
            Input::IconFound {
                account,
                raw,
                page_url,
            } => self.icon_found(account, &raw, page_url.as_deref()),
            Input::IconFetched { account, data_url } => self.icon_fetched(account, data_url),
            Input::SimpleIconFetched {
                account,
                slug,
                result,
            } => self.simple_icon_fetched(account, &slug, result),
            Input::ServicesLoaded { defs, errors } => self.services_loaded(defs, errors),
        }
    }

    fn ui(&mut self, command: UiCommand) -> Vec<Effect> {
        match command {
            UiCommand::Select { account } => self.select(account),
            UiCommand::AddAccount {
                service,
                label,
                instance,
            } => self.add_account(&service, &label, instance),
            UiCommand::RemoveAccount { account } => self.remove_account(account),
            UiCommand::SetMuted { account, muted } => self.set_muted(account, muted),
            UiCommand::Move { account, delta } => self.move_account(account, delta),
            UiCommand::Reload { account } => self.reload(account),
            UiCommand::ForceReload { account } => self.force_reload(account),
            UiCommand::SetDialog { open } => self.set_dialog(open),
            UiCommand::SetDnd { enabled } => self.set_dnd(enabled),
            UiCommand::UpdateSettings { settings } => self.update_settings(settings),
            UiCommand::UpdateAccount { account, settings } => {
                self.update_account(account, settings)
            }
            UiCommand::GoBack { account } => self.go_back(account),
            UiCommand::GoForward { account } => self.go_forward(account),
            UiCommand::OpenInBrowser { account } => self.open_in_browser(account),
            UiCommand::ClearData { account } => self.clear_data(account),
            UiCommand::Hibernate { account } => self.hibernate(account),
            UiCommand::RefreshIcon { account } => self.refresh_icon(account),
            UiCommand::AddCustomService { service } => self.add_custom_service(&service),
            UiCommand::UpdateCustomService { service } => self.update_custom_service(&service),
            UiCommand::RemoveCustomService { id } => self.remove_custom_service(&id),
            UiCommand::OpenServicesFolder => vec![Effect::OpenServicesFolder],
            UiCommand::ReloadServices => vec![Effect::LoadServices],
            UiCommand::OpenProjectPage => self.open_project_page(),
        }
    }

    fn settings(&self) -> &AppSettings {
        &self.persisted.settings.app
    }

    fn persist(&mut self) -> Effect {
        self.persisted.settings.active = self.active;
        Effect::Persist(self.persisted.clone())
    }

    fn emit(&self) -> Effect {
        Effect::EmitSnapshot(self.snapshot())
    }

    fn has_account(&self, account_id: Uuid) -> bool {
        self.account(account_id).is_some()
    }

    fn account(&self, account_id: Uuid) -> Option<&Account> {
        self.persisted
            .accounts
            .iter()
            .find(|account| account.id == account_id)
    }

    fn account_mut(&mut self, account_id: Uuid) -> Option<&mut Account> {
        self.persisted
            .accounts
            .iter_mut()
            .find(|account| account.id == account_id)
    }

    /// Position of the account in the sidebar.
    fn account_index(&self, account_id: Uuid) -> Option<usize> {
        self.persisted
            .accounts
            .iter()
            .position(|account| account.id == account_id)
    }

    fn is_loaded(&self, account_id: Uuid) -> bool {
        self.loaded_views.contains(&account_id)
    }

    /// Accounts with a view, in sidebar order.
    fn loaded_account_ids(&self) -> Vec<Uuid> {
        self.persisted
            .accounts
            .iter()
            .map(|account| account.id)
            .filter(|&id| self.is_loaded(id))
            .collect()
    }

    fn unknown(account_id: Uuid) -> Effect {
        Effect::Log(format!("unknown account {account_id}"))
    }
}

/// One log effect per message.
fn log_effects(messages: Vec<String>) -> Vec<Effect> {
    messages.into_iter().map(Effect::Log).collect()
}

/// Trims an optional text setting; empty means unset.
fn trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}
