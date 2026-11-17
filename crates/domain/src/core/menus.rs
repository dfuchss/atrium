//! Native app, tray and account menus: their commands and their content.

use uuid::Uuid;

use super::settings::ZOOM_STEPS;
use super::{
    AccountMenu, AccountMenuAction, AccountMenuItem, Core, DockBadge, Effect, MenuAccount,
    MenuAction, MenuModel,
};

/// Tolerance when looking for the next zoom step, so 0.67 counts as the 0.67 step.
const ZOOM_STEP_TOLERANCE: f64 = 1e-3;

impl Core {
    /// Menu commands act on the active account, except Do Not Disturb.
    pub(super) fn menu(&mut self, action: MenuAction) -> Vec<Effect> {
        match (action, self.active) {
            (MenuAction::ToggleDnd, _) => self.set_dnd(!self.settings().dnd),
            (_, None) => vec![Effect::Log(format!("menu {action:?}: no active account"))],
            (MenuAction::Reload, Some(active)) => self.reload(active),
            (MenuAction::Back, Some(active)) => self.go_back(active),
            (MenuAction::Forward, Some(active)) => self.go_forward(active),
            (MenuAction::OpenInBrowser, Some(active)) => self.open_in_browser(active),
            (MenuAction::ZoomIn, Some(active)) => self.step_zoom(active, zoom_in_step),
            (MenuAction::ZoomOut, Some(active)) => self.step_zoom(active, zoom_out_step),
            (MenuAction::ZoomReset, Some(active)) => self.set_zoom(active, 1.0),
        }
    }

    /// The menu of an account's sidebar icon: Sleep for a loaded background account (the
    /// shown one stays awake), otherwise Wake Up for an enabled one.
    pub(super) fn show_account_menu(&self, account_id: Uuid) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        let loaded = self.is_loaded(account_id);
        let item = |action, enabled| AccountMenuItem { action, enabled };
        let first = if loaded {
            item(AccountMenuAction::Sleep, self.active != Some(account_id))
        } else {
            item(AccountMenuAction::WakeUp, account.enabled)
        };
        vec![Effect::ShowAccountMenu(AccountMenu {
            account: account_id,
            items: vec![
                first,
                item(AccountMenuAction::Settings, true),
                item(AccountMenuAction::Reload, loaded),
            ],
        })]
    }

    pub(super) fn account_menu(
        &mut self,
        account_id: Uuid,
        action: AccountMenuAction,
    ) -> Vec<Effect> {
        match action {
            AccountMenuAction::Sleep => self.hibernate(account_id),
            AccountMenuAction::WakeUp => self.wake_up(account_id),
            AccountMenuAction::Settings => self.open_account_settings(account_id),
            AccountMenuAction::Reload => self.reload(account_id),
        }
    }

    /// Loads a sleeping account's view in the background; selecting it is still up to the user.
    fn wake_up(&mut self, account_id: Uuid) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        if !account.enabled || self.is_loaded(account_id) {
            return vec![Effect::Log(format!(
                "wake up: account {account_id} is disabled or awake"
            ))];
        }
        let mut effects = self.load_view(account_id, self.is_visible(account_id));
        self.record_activity(account_id);
        effects.push(self.emit());
        effects
    }

    /// Opens the settings dialog with the account's settings.
    fn open_account_settings(&mut self, account_id: Uuid) -> Vec<Effect> {
        if !self.has_account(account_id) {
            return vec![Self::unknown(account_id)];
        }
        self.edit_account = Some(account_id);
        let effects = self.set_dialog(true);
        self.edit_account = None;
        effects
    }

    /// Applies the zoom step `next` picks for the current zoom; none at the end of the scale.
    fn step_zoom(&mut self, account_id: Uuid, next: fn(f64) -> Option<f64>) -> Vec<Effect> {
        let current = self.account(account_id).map_or(1.0, |account| account.zoom);
        match next(current) {
            Some(zoom) => self.set_zoom(account_id, zoom),
            None => vec![],
        }
    }

    /// `UpdateMenus` once started and whenever the menu content changed.
    pub(super) fn menus_if_changed(&mut self) -> Option<Effect> {
        if !self.started {
            return None;
        }
        let menus = self.menu_model();
        if self.last_menus.as_ref() == Some(&menus) {
            return None;
        }
        self.last_menus = Some(menus.clone());
        Some(Effect::UpdateMenus(menus))
    }

    fn menu_model(&self) -> MenuModel {
        let tray_title = match self.badge() {
            DockBadge::Count(count) => count.to_string(),
            DockBadge::Dot => "•".to_string(),
            DockBadge::None => String::new(),
        };
        MenuModel {
            accounts: self
                .persisted
                .accounts
                .iter()
                .filter(|account| account.enabled)
                .map(|account| MenuAccount {
                    id: account.id,
                    label: account.label.clone(),
                    unread: self.unread.of(account.id).direct,
                })
                .collect(),
            dnd: self.settings().dnd,
            show_tray: self.settings().show_tray,
            tray_title,
        }
    }
}

fn zoom_in_step(current: f64) -> Option<f64> {
    ZOOM_STEPS
        .into_iter()
        .find(|&step| step > current + ZOOM_STEP_TOLERANCE)
}

fn zoom_out_step(current: f64) -> Option<f64> {
    ZOOM_STEPS
        .into_iter()
        .rev()
        .find(|&step| step < current - ZOOM_STEP_TOLERANCE)
}
