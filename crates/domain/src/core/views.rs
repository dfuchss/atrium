//! Service views: what they are made of, their lifecycle, and commands acting on one view.

use uuid::Uuid;

use super::{Core, Effect, ICON_SCRIPT, ViewPolicy, ViewSpec, scripts};
use crate::model::Account;

impl Core {
    pub(super) fn view_spec(&self, account: &Account) -> Result<ViewSpec, String> {
        let service = self.catalog.get(&account.service).ok_or_else(|| {
            format!(
                "account {}: unknown service {}",
                account.id, account.service
            )
        })?;
        let instance = account.instance.as_deref();
        let url = service
            .resolve_url(instance)
            .map_err(|error| format!("account {}: {error}", account.id))?;
        let user_agent = match &account.user_agent {
            Some(preset) => crate::catalog::resolve_user_agent_preset(Some(preset)),
            None => service.resolve_user_agent(),
        };
        Ok(ViewSpec {
            account: account.id,
            url,
            store_id: account.store_id(),
            user_agent,
            init_script: scripts::init_script(service, account),
            internal_hosts: service.resolve_hosts(instance),
        })
    }

    pub(super) fn view_policy(&self, account: &Account) -> ViewPolicy {
        let service_allows_media = self
            .catalog
            .get(&account.service)
            .is_some_and(|service| service.allow_media);
        ViewPolicy {
            link_target: account.link_target.unwrap_or(self.settings().link_target),
            allow_media: account.allow_media.unwrap_or(service_allows_media),
            app_hosts: account.app_hosts.clone(),
        }
    }

    /// `SetViewPolicy`, `CreateView`, `SetZoom` for an account (marks it loaded), or a log line.
    pub(super) fn load_view(&mut self, account_id: Uuid, visible: bool) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        match self.view_spec(account) {
            Ok(spec) => {
                let (policy, zoom) = (self.view_policy(account), account.zoom);
                self.loaded_views.insert(account_id);
                vec![
                    Effect::SetViewPolicy {
                        account: account_id,
                        policy,
                    },
                    Effect::CreateView {
                        spec,
                        bounds: self.content_bounds(),
                        visible,
                    },
                    Effect::SetZoom {
                        account: account_id,
                        zoom,
                    },
                ]
            }
            Err(error) => vec![Effect::Log(error)],
        }
    }

    /// `Destroy` if the account has a view.
    pub(super) fn unload_view(&mut self, account_id: Uuid) -> Vec<Effect> {
        if self.loaded_views.remove(&account_id) {
            vec![Effect::Destroy(account_id)]
        } else {
            vec![]
        }
    }

    /// Replaces the account's view by a new one, e.g. after its URL or init script changed.
    pub(super) fn recreate_view(&mut self, account_id: Uuid) -> Vec<Effect> {
        let mut effects = self.unload_view(account_id);
        effects.extend(self.load_view(account_id, self.is_visible(account_id)));
        effects
    }

    /// Runs `action` for an account with a view; logs unknown or unloaded accounts.
    fn with_view(
        &self,
        account_id: Uuid,
        command: &str,
        action: impl FnOnce(Uuid) -> Effect,
    ) -> Vec<Effect> {
        if !self.has_account(account_id) {
            vec![Self::unknown(account_id)]
        } else if !self.is_loaded(account_id) {
            vec![Effect::Log(format!(
                "{command}: account {account_id} has no view (disabled or hibernated)"
            ))]
        } else {
            vec![action(account_id)]
        }
    }

    pub(super) fn reload(&self, account_id: Uuid) -> Vec<Effect> {
        self.with_view(account_id, "reload", Effect::Reload)
    }

    pub(super) fn go_back(&self, account_id: Uuid) -> Vec<Effect> {
        self.with_view(account_id, "back", |account| Effect::Eval {
            account,
            script: scripts::HISTORY_BACK.into(),
        })
    }

    pub(super) fn go_forward(&self, account_id: Uuid) -> Vec<Effect> {
        self.with_view(account_id, "forward", |account| Effect::Eval {
            account,
            script: scripts::HISTORY_FORWARD.into(),
        })
    }

    pub(super) fn refresh_icon(&self, account_id: Uuid) -> Vec<Effect> {
        self.with_view(account_id, "refresh icon", |account| Effect::FindIcon {
            account,
            script: ICON_SCRIPT.into(),
        })
    }

    /// Opens the page shown in the view, or the service's start page if there is no view.
    pub(super) fn open_in_browser(&self, account_id: Uuid) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        let browser = self.settings().browser.clone();
        if self.is_loaded(account_id) {
            return vec![Effect::OpenInBrowser {
                account: account_id,
                browser,
            }];
        }
        match self.view_spec(account) {
            // Service URLs are always http(s) (checked by `resolve_url`).
            Ok(spec) => vec![Effect::OpenExternal {
                url: spec.url.to_string(),
                browser,
            }],
            Err(error) => vec![Effect::Log(error)],
        }
    }

    /// Clears cookies and storage: through the view if it exists, else on disk.
    pub(super) fn clear_data(&self, account_id: Uuid) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        let clear = if self.is_loaded(account_id) {
            Effect::ClearData(account_id)
        } else {
            Effect::DeleteProfile {
                store_id: account.store_id(),
            }
        };
        vec![
            Effect::Log(format!("clearing browsing data of account {account_id}")),
            clear,
        ]
    }

    pub(super) fn notification_clicked(&mut self, account_id: Uuid, shim_id: u64) -> Vec<Effect> {
        let mut effects = self.select(account_id);
        if self.has_account(account_id) && self.is_loaded(account_id) {
            effects.push(Effect::FocusWindow);
            effects.push(Effect::Eval {
                account: account_id,
                script: scripts::notification_click_script(shim_id),
            });
        }
        effects
    }

    /// A new window the adapter sent to the browser; only safe schemes are opened.
    pub(super) fn open_external_link(&self, account_id: Uuid, url: String) -> Vec<Effect> {
        if crate::security::opener_url_allowed(&url) {
            vec![Effect::OpenExternal {
                url,
                browser: self.settings().browser.clone(),
            }]
        } else {
            vec![Effect::Log(format!(
                "[{account_id}] not opening {}: scheme not allowed",
                crate::security::redact_url(&url)
            ))]
        }
    }

    pub(super) fn open_project_page(&self) -> Vec<Effect> {
        vec![Effect::OpenExternal {
            url: protocol::PROJECT_URL.into(),
            browser: self.settings().browser.clone(),
        }]
    }
}
