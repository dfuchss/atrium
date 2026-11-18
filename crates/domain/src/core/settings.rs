//! App and account settings changed in the settings dialog or the menus.

use protocol::{AccountSettings, AppSettings};
use uuid::Uuid;

use super::account_label;
use super::icons::normalize_slug;
use super::{Core, Effect};
use crate::catalog::normalized_instance;
use crate::model::Account;
use crate::trimmed;

/// Zoom levels of "Zoom In" / "Zoom Out".
pub(super) const ZOOM_STEPS: [f64; 10] = [0.5, 0.67, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];
const ZOOM_MIN: f64 = 0.25;
const ZOOM_MAX: f64 = 5.0;

impl Core {
    pub(super) fn update_settings(&mut self, mut settings: AppSettings) -> Vec<Effect> {
        settings.browser = trimmed(settings.browser.as_deref());
        let previous = std::mem::replace(&mut self.persisted.settings.app, settings);
        let mut effects = vec![
            self.persist(),
            Effect::ApplySystemSettings(self.settings().clone()),
        ];
        if previous.theme != self.settings().theme {
            effects.push(Effect::ApplyTheme(self.settings().theme));
        }
        if self.started && previous.icon_size != self.settings().icon_size {
            effects.extend(self.layout());
        }
        if previous.link_target != self.settings().link_target {
            effects.extend(self.default_link_target_policies());
        }
        effects.extend(self.dock_badge());
        effects.push(self.emit());
        effects
    }

    /// New view policies of the loaded accounts that follow the global link target.
    fn default_link_target_policies(&self) -> Vec<Effect> {
        self.persisted
            .accounts
            .iter()
            .filter(|account| account.settings.link_target.is_none() && self.is_loaded(account.id))
            .map(|account| Effect::SetViewPolicy {
                account: account.id,
                policy: self.view_policy(account),
            })
            .collect()
    }

    pub(super) fn set_dnd(&mut self, enabled: bool) -> Vec<Effect> {
        self.persisted.settings.app.dnd = enabled;
        vec![self.persist(), self.emit()]
    }

    pub(super) fn set_zoom(&mut self, account_id: Uuid, zoom: f64) -> Vec<Effect> {
        let zoom = sanitize_zoom(zoom);
        let Some(account) = self.account_mut(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        if account.settings.zoom == zoom {
            return vec![];
        }
        account.settings.zoom = zoom;
        let mut effects = vec![];
        if self.is_loaded(account_id) {
            effects.push(Effect::SetZoom {
                account: account_id,
                zoom,
            });
        }
        effects.push(self.persist());
        effects.push(self.emit());
        effects
    }

    /// Validates and stores the account's settings, then adapts its view: enables, disables,
    /// recreates it, or updates its policy and zoom in place.
    pub(super) fn update_account(
        &mut self,
        account_id: Uuid,
        settings: AccountSettings,
    ) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        let before = account.clone();
        let (settings, problems) = match self.normalized_account_settings(&before, settings) {
            Ok(normalized) => normalized,
            Err(error) => {
                return self.rejected(format!("update account {account_id}: {error}"));
            }
        };
        let new_slug = settings.simple_icon.clone();
        if let Some(account) = self.account_mut(account_id) {
            account.apply_settings(settings);
        }
        let mut effects = self.view_effects_after_update(&before);
        effects.extend(
            problems
                .into_iter()
                .map(|problem| Effect::Log(format!("update account {account_id}: {problem}"))),
        );
        if self.started
            && before.settings.simple_icon != new_slug
            && let Some(slug) = new_slug
        {
            effects.push(Effect::FetchSimpleIcon {
                account: account_id,
                slug,
            });
        }
        effects.push(self.persist());
        effects.extend(self.dock_badge());
        effects.push(self.emit());
        effects
    }

    /// Normalised settings plus the problems of fields that were dropped (invalid slug or app
    /// hosts); an error if the instance does not fit the service.
    pub(super) fn normalized_account_settings(
        &self,
        account: &Account,
        mut settings: AccountSettings,
    ) -> Result<(AccountSettings, Vec<String>), String> {
        let service = self
            .registry
            .get(&account.service)
            .ok_or_else(|| format!("unknown service {}", account.service))?;
        settings.instance = normalized_instance(settings.instance.as_deref());
        service
            .resolve_url(settings.instance.as_deref())
            .map_err(|error| error.to_string())?;
        let mut problems = vec![];
        settings.label = account_label(&settings.label, &service.name);
        settings.user_agent = trimmed(settings.user_agent.as_deref());
        settings.custom_css = settings.custom_css.filter(|css| !css.trim().is_empty());
        settings.custom_icon = trimmed(settings.custom_icon.as_deref());
        settings.zoom = sanitize_zoom(settings.zoom);
        settings.simple_icon = normalize_slug(settings.simple_icon.as_deref().unwrap_or(""))
            .unwrap_or_else(|error| {
                problems.push(error);
                None
            });
        let (app_hosts, rejected_hosts) = crate::catalog::normalize_app_hosts(&settings.app_hosts);
        settings.app_hosts = app_hosts;
        problems.extend(
            rejected_hosts
                .iter()
                .map(|host| format!("ignoring invalid app host {host:?}")),
        );
        Ok((settings, problems))
    }

    /// View effects of an account whose settings changed from `before`. Before the start
    /// there are none: views are created on start with the new settings.
    fn view_effects_after_update(&mut self, before: &Account) -> Vec<Effect> {
        let account_id = before.id;
        let Some(after) = self.account(account_id) else {
            return vec![];
        };
        if !self.started {
            return vec![];
        }
        let needs_new_view = before.settings.instance != after.settings.instance
            || before.settings.user_agent != after.settings.user_agent
            || before.settings.custom_css != after.settings.custom_css;
        match (before.settings.enabled, after.settings.enabled) {
            (true, false) => self.disable_account(account_id),
            (false, true) => self.enable_account(account_id),
            _ if !self.is_loaded(account_id) => vec![],
            _ if needs_new_view => self.recreate_view(account_id),
            _ => self.update_view_in_place(before),
        }
    }

    fn disable_account(&mut self, account_id: Uuid) -> Vec<Effect> {
        let mut effects = self.unload_view(account_id);
        self.unread.forget(account_id);
        if self.active == Some(account_id) {
            let index = self.account_index(account_id).unwrap_or(0);
            self.active = self.nearest_enabled_account(index, Some(account_id));
            effects.extend(self.show_active());
        }
        effects
    }

    fn enable_account(&mut self, account_id: Uuid) -> Vec<Effect> {
        if self.active.is_none() {
            self.active = Some(account_id);
        }
        self.load_view(account_id, self.is_visible(account_id))
    }

    /// Policy and zoom changes that need no new view.
    fn update_view_in_place(&self, before: &Account) -> Vec<Effect> {
        let Some(after) = self.account(before.id) else {
            return vec![];
        };
        let mut effects = vec![];
        let policy = self.view_policy(after);
        if self.view_policy(before) != policy {
            effects.push(Effect::SetViewPolicy {
                account: after.id,
                policy,
            });
        }
        if before.settings.zoom != after.settings.zoom {
            effects.push(Effect::SetZoom {
                account: after.id,
                zoom: after.settings.zoom,
            });
        }
        effects
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
