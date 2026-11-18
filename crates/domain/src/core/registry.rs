//! The known services: built-ins, the user's TOML files and the custom services, with their
//! compiled unread title patterns.

use std::collections::HashMap;

use protocol::CustomService;

use super::unread::TitleRules;
use crate::catalog::{Catalog, ServiceDef};

pub(super) struct ServiceRegistry {
    /// The catalog without user files (built-ins only), the base of every reload.
    builtin: Catalog,
    /// Built-in + user TOML services; `catalog` = this + custom services.
    base: Catalog,
    /// The user's TOML definitions applied on top of `builtin` in `base`.
    user_defs: Vec<ServiceDef>,
    /// Problems of the last load of the user's service files.
    errors: Vec<String>,
    catalog: Catalog,
    /// Unread title patterns per service id, compiled from `catalog`.
    title_rules: HashMap<String, TitleRules>,
}

impl ServiceRegistry {
    /// Only `builtin` until [`ServiceRegistry::rebuild`] adds the custom services.
    pub(super) fn new(builtin: Catalog) -> ServiceRegistry {
        ServiceRegistry {
            catalog: builtin.clone(),
            base: builtin.clone(),
            builtin,
            user_defs: vec![],
            errors: vec![],
            title_rules: HashMap::new(),
        }
    }

    pub(super) fn get(&self, id: &str) -> Option<&ServiceDef> {
        self.catalog.get(id)
    }

    /// In catalog order.
    pub(super) fn services(&self) -> &[ServiceDef] {
        self.catalog.services()
    }

    pub(super) fn title_rules(&self, service_id: &str) -> Option<&TitleRules> {
        self.title_rules.get(service_id)
    }

    /// Problems of the last load of the user's service files.
    pub(super) fn errors(&self) -> &[String] {
        &self.errors
    }

    /// True if a built-in or user TOML service has this id.
    pub(super) fn defines_file_service(&self, id: &str) -> bool {
        self.base.get(id).is_some()
    }

    /// Replaces the user definitions (a failed file keeps the previous definitions of ids that
    /// no file defines any more, as the failed file may be the one that defined them) and
    /// rebuilds the catalog. Returns the problems of the custom services.
    pub(super) fn apply_user_services(
        &mut self,
        mut defs: Vec<ServiceDef>,
        errors: Vec<String>,
        custom_services: &[CustomService],
    ) -> Vec<String> {
        if !errors.is_empty() {
            let kept: Vec<ServiceDef> = self
                .user_defs
                .iter()
                .filter(|old| !defs.iter().any(|def| def.id == old.id))
                .cloned()
                .collect();
            defs.extend(kept);
        }
        let mut base = self.builtin.clone();
        for def in &defs {
            base.insert(def.clone());
        }
        self.user_defs = defs;
        self.base = base;
        self.errors = errors;
        self.rebuild(custom_services)
    }

    /// Catalog = base + valid custom services; recompiles the unread rules. Returns problems.
    pub(super) fn rebuild(&mut self, custom_services: &[CustomService]) -> Vec<String> {
        let mut catalog = self.base.clone();
        let mut problems = vec![];
        for custom in custom_services {
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
}
