//! The [`Snapshot`] the sidebar UI renders.

use protocol::{AccountView, ServiceInfo, Snapshot};

use super::Core;
use crate::catalog::ServiceDef;
use crate::model::Account;

impl Core {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            accounts: self
                .persisted
                .accounts
                .iter()
                .map(|account| self.account_view(account))
                .collect(),
            active: self.active,
            services: self
                .catalog
                .services()
                .iter()
                .map(|service| self.service_info(service))
                .collect(),
            // Colors as the catalog uses them: a hand-edited accounts.json must not reach the
            // UI's style attributes either.
            custom_services: self
                .persisted
                .custom_services
                .iter()
                .map(|service| protocol::CustomService {
                    color: crate::catalog::color_or_default(&service.color),
                    ..service.clone()
                })
                .collect(),
            dialog_open: self.dialog_open,
            settings: self.settings().clone(),
            app: self.app_info.clone(),
            service_errors: self.service_errors.clone(),
        }
    }

    fn account_view(&self, account: &Account) -> AccountView {
        let service = self.catalog.get(&account.service);
        let service_name =
            service.map_or_else(|| account.service.clone(), |service| service.name.clone());
        let unread = self.unread.of(account.id);
        let initials_source = if account.label == service_name {
            &service_name
        } else {
            &account.label
        };
        let (icon, icon_glyph) = sidebar_icon(account);
        AccountView {
            id: account.id,
            service: account.service.clone(),
            initials: initials(initials_source),
            service_name,
            color: service.map_or_default(|service| service.color.clone()),
            unread: unread.direct,
            indirect: unread.indirect,
            instance_label: service.and_then(|service| service.instance_label.clone()),
            icon,
            icon_glyph,
            hibernated: !account.enabled || (self.started && !self.is_loaded(account.id)),
            settings: account.settings(),
        }
    }

    fn service_info(&self, service: &ServiceDef) -> ServiceInfo {
        ServiceInfo {
            id: service.id.clone(),
            name: service.name.clone(),
            color: service.color.clone(),
            instance_label: service.instance_label.clone(),
            instance_default: service.instance_default.clone(),
            custom: self.is_custom(&service.id),
            allow_media: service.allow_media,
            url: service.url.clone(),
            title_regex: service.unread.title_regex.clone(),
            user_agent: service.user_agent.clone(),
        }
    }
}

/// The sidebar icon and whether it is a Simple Icons glyph: the custom icon wins, then a cached
/// glyph, then the favicon.
fn sidebar_icon(account: &Account) -> (Option<String>, bool) {
    let glyph = account.custom_icon.is_none()
        && account.simple_icon.is_some()
        && account.simple_icon_data.is_some();
    if glyph {
        (account.simple_icon_data.clone(), true)
    } else {
        let icon = account
            .custom_icon
            .clone()
            .or_else(|| account.favicon.clone());
        (icon, false)
    }
}

/// Two letters for an icon without image: the initials of the first two words, or the first
/// two letters of a single word.
pub(super) fn initials(text: &str) -> String {
    let first_upper = |word: &str| -> String {
        word.chars()
            .next()
            .map_or_else(String::new, |first| first.to_uppercase().collect())
    };
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.as_slice() {
        [] => String::new(),
        [word] => {
            let second: String = word.chars().skip(1).take(1).collect();
            first_upper(word) + &second
        }
        [first, second, ..] => first_upper(first) + &first_upper(second),
    }
}
