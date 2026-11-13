//! The service catalog: built-ins, the user's TOML files and custom services.

use protocol::CustomService;
use uuid::Uuid;

use super::unread::TitleRules;
use super::{Core, Effect, ViewSpec, log_effects};
use crate::catalog::ServiceDef;

impl Core {
    /// Applies the user's service files found at startup (call before `Input::Started`).
    /// Their problems are logged on start and shown in the snapshot.
    pub fn set_user_services(&mut self, defs: Vec<ServiceDef>, errors: Vec<String>) {
        self.startup_log.extend(errors.iter().cloned());
        // Problems of the custom services are already in the startup log (see `new`).
        self.apply_user_services(defs, errors);
    }

    /// Replaces the user definitions (a failed file keeps the previous definitions of ids that
    /// no file defines any more, as the failed file may be the one that defined them) and
    /// rebuilds the catalog. Returns the problems of the custom services.
    fn apply_user_services(
        &mut self,
        mut defs: Vec<ServiceDef>,
        errors: Vec<String>,
    ) -> Vec<String> {
        if !errors.is_empty() {
            let kept: Vec<ServiceDef> = self
                .user_service_defs
                .iter()
                .filter(|old| !defs.iter().any(|def| def.id == old.id))
                .cloned()
                .collect();
            defs.extend(kept);
        }
        let mut base = self.builtin_catalog.clone();
        for def in &defs {
            base.insert(def.clone());
        }
        self.user_service_defs = defs;
        self.base_catalog = base;
        self.service_errors = errors;
        self.rebuild_catalog()
    }

    /// Reload of the user's files: new catalog and unread rules; accounts whose view would now
    /// differ are recreated, the view policies are re-sent.
    pub(super) fn services_loaded(
        &mut self,
        defs: Vec<ServiceDef>,
        errors: Vec<String>,
    ) -> Vec<Effect> {
        let old_specs: Vec<(Uuid, Option<ViewSpec>)> = self
            .loaded_account_ids()
            .into_iter()
            .map(|id| (id, self.account(id).and_then(|a| self.view_spec(a).ok())))
            .collect();
        let mut effects = log_effects(errors.clone());
        effects.push(Effect::Log(format!(
            "reloaded services: {} user definitions, {} errors",
            defs.len(),
            errors.len()
        )));
        effects.extend(log_effects(self.apply_user_services(defs, errors)));
        for (account_id, old_spec) in old_specs {
            effects.extend(self.refresh_view(account_id, old_spec.as_ref()));
        }
        effects.push(self.emit());
        effects
    }

    /// Recreates the view if its spec changed with the catalog, else re-sends its policy.
    fn refresh_view(&mut self, account_id: Uuid, old_spec: Option<&ViewSpec>) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![];
        };
        let new_spec = self.view_spec(account).ok();
        if new_spec.is_some() && new_spec.as_ref() != old_spec {
            self.recreate_view(account_id)
        } else {
            vec![Effect::SetViewPolicy {
                account: account_id,
                policy: self.view_policy(account),
            }]
        }
    }

    /// Catalog = base + valid custom services; recompiles the unread rules. Returns problems.
    pub(super) fn rebuild_catalog(&mut self) -> Vec<String> {
        let mut catalog = self.base_catalog.clone();
        let mut problems = vec![];
        for custom in &self.persisted.custom_services {
            match ServiceDef::from_custom(custom) {
                Ok(def) => catalog.insert(def),
                Err(problem) => problems.push(problem),
            }
        }
        self.title_rules = catalog
            .services()
            .iter()
            .map(|service| (service.id.clone(), TitleRules::compile(service)))
            .collect();
        self.catalog = catalog;
        problems
    }

    pub(super) fn is_custom(&self, service_id: &str) -> bool {
        self.persisted
            .custom_services
            .iter()
            .any(|custom| custom.id == service_id)
    }

    fn custom_service_index(&self, service_id: &str) -> Option<usize> {
        self.persisted
            .custom_services
            .iter()
            .position(|custom| custom.id == service_id)
    }

    fn custom_service_error(&self, message: String) -> Vec<Effect> {
        vec![Effect::Log(message), self.emit()]
    }

    /// Persists and emits after a custom service change (the catalog is already rebuilt).
    fn custom_services_changed(&mut self, mut effects: Vec<Effect>) -> Vec<Effect> {
        effects.push(self.persist());
        effects.push(self.emit());
        effects
    }

    pub(super) fn add_custom_service(&mut self, service: &CustomService) -> Vec<Effect> {
        let mut custom = match crate::catalog::normalize_custom(service) {
            Ok(custom) => custom,
            Err(error) => return self.custom_service_error(format!("add custom service: {error}")),
        };
        custom.id = crate::catalog::custom_service_id(&custom.name, |id| {
            self.base_catalog.get(id).is_some() || self.is_custom(id)
        });
        let id = custom.id.clone();
        self.persisted.custom_services.push(custom);
        let mut effects = log_effects(self.rebuild_catalog());
        effects.push(Effect::Log(format!("added custom service {id}")));
        self.custom_services_changed(effects)
    }

    /// Stores the edited service and recreates the views of its accounts.
    pub(super) fn update_custom_service(&mut self, service: &CustomService) -> Vec<Effect> {
        let Some(index) = self.custom_service_index(&service.id) else {
            return self.custom_service_error(format!(
                "update custom service: unknown id {:?}",
                service.id
            ));
        };
        let custom = match crate::catalog::normalize_custom(service) {
            Ok(custom) => custom,
            Err(error) => {
                return self.custom_service_error(format!("update custom service: {error}"));
            }
        };
        let id = custom.id.clone();
        self.persisted.custom_services[index] = custom;
        let mut effects = log_effects(self.rebuild_catalog());
        let users: Vec<Uuid> = self
            .persisted
            .accounts
            .iter()
            .filter(|account| account.service == id && self.is_loaded(account.id))
            .map(|account| account.id)
            .collect();
        for account_id in users {
            effects.extend(self.recreate_view(account_id));
        }
        self.custom_services_changed(effects)
    }

    /// Only unused services can be removed.
    pub(super) fn remove_custom_service(&mut self, id: &str) -> Vec<Effect> {
        let Some(index) = self.custom_service_index(id) else {
            return self.custom_service_error(format!("remove custom service: unknown id {id:?}"));
        };
        let users = self
            .persisted
            .accounts
            .iter()
            .filter(|account| account.service == id)
            .count();
        if users > 0 {
            return self.custom_service_error(format!(
                "remove custom service {id}: still used by {users} account(s)"
            ));
        }
        self.persisted.custom_services.remove(index);
        let effects = log_effects(self.rebuild_catalog());
        self.custom_services_changed(effects)
    }
}
