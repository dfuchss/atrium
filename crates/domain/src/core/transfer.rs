//! The settings and the full import. The app handles the export file itself.

use super::{Core, Effect, log_effects};
use crate::model::Persisted;

impl Core {
    /// Applies the app settings and the custom services of `saved`: a custom service of the
    /// same id is updated, any other added. Accounts stay as they are. Saves and emits once.
    pub(super) fn import_settings(&mut self, saved: Persisted) -> Vec<Effect> {
        let mut effects = vec![];
        let mut updated = vec![];
        for service in &saved.custom_services {
            let custom = match crate::catalog::normalize_custom(service) {
                Ok(custom) => custom,
                Err(error) => {
                    effects.push(Effect::Log(format!("import settings: {error}")));
                    continue;
                }
            };
            if let Some(index) = self.custom_service_index(&custom.id) {
                if self.persisted.custom_services[index] != custom {
                    updated.push(custom.id.clone());
                    self.persisted.custom_services[index] = custom;
                }
            } else {
                self.push_custom_service(custom);
            }
        }
        effects.extend(log_effects(self.rebuild_catalog()));
        for id in updated {
            effects.extend(self.recreate_views_of_service(&id));
        }
        effects.extend(self.update_settings(saved.settings.app));
        effects.push(Effect::Log("imported settings".into()));
        effects
    }

    /// Replaces the saved state by `saved`, without touching the views: the app restarts to
    /// load it. Later saves of this run save it again instead of the state it replaced.
    pub(super) fn import_everything(&mut self, saved: Persisted) -> Vec<Effect> {
        self.active = saved.settings.active;
        self.persisted = saved;
        vec![
            self.persist(),
            Effect::Log("imported everything".into()),
            Effect::Restart,
        ]
    }
}
