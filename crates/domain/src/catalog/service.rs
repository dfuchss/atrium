//! A service definition, and the URL, hosts and user agent it resolves to for an account's
//! instance value.

use protocol::{CustomService, INSTANCE_PLACEHOLDER};
use serde::Deserialize;

use super::CatalogError;
use super::custom::{DEFAULT_TITLE_REGEX, normalize_custom};
use super::hosts::url_host_pattern;
use super::user_agent::resolve_user_agent_preset;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ServiceDef {
    pub id: String,
    pub name: String,
    pub color: String,
    pub url: String,
    #[serde(default)]
    pub instance_label: Option<String>,
    #[serde(default)]
    pub instance_default: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub internal_hosts: Vec<String>,
    /// Default for granting camera/microphone/screen capture (call apps); accounts can override.
    #[serde(default)]
    pub allow_media: bool,
    /// `localStorage` keys removed before the page's own scripts run, e.g. stale single-instance
    /// session locks (Element) left behind when the app was quit without unloading the page.
    /// Only one page per account exists in this app, so such locks are always stale on load.
    #[serde(default)]
    pub clear_local_storage: Vec<String>,
    #[serde(default)]
    pub unread: UnreadRules,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct UnreadRules {
    #[serde(default)]
    pub title_regex: Option<String>,
    #[serde(default)]
    pub title_indirect_regex: Option<String>,
    #[serde(default)]
    pub poll: Option<String>,
}

/// Trims an instance value and strips a pasted scheme and trailing slashes; empty means none.
pub(crate) fn normalized_instance(instance: Option<&str>) -> Option<String> {
    instance
        .map(normalize_instance)
        .filter(|instance| !instance.is_empty())
}

fn normalize_instance(instance: &str) -> String {
    let trimmed = instance.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    without_scheme.trim_end_matches('/').to_string()
}

impl ServiceDef {
    fn effective_instance(&self, instance: Option<&str>) -> Option<String> {
        normalized_instance(instance)
            .or_else(|| normalized_instance(self.instance_default.as_deref()))
    }

    /// URL with `{instance}` replaced. Errors if the URL needs an instance and none (and no default) is given,
    /// or if the result is not a valid http(s) URL.
    pub fn resolve_url(&self, instance: Option<&str>) -> Result<url::Url, CatalogError> {
        let url = self.url.clone();
        let raw = if url.contains(INSTANCE_PLACEHOLDER) {
            let instance = self
                .effective_instance(instance)
                .ok_or_else(|| CatalogError::MissingInstance(self.id.clone()))?;
            url.replace(INSTANCE_PLACEHOLDER, &instance)
        } else {
            url
        };
        let invalid = || CatalogError::InvalidUrl {
            service: self.id.clone(),
            url: raw.clone(),
        };
        let parsed = url::Url::parse(&raw).map_err(|_| invalid())?;
        match parsed.scheme() {
            "http" | "https" => Ok(parsed),
            _ => Err(invalid()),
        }
    }

    /// `internal_hosts` with `{instance}` replaced (instance = host part of the instance value).
    pub fn resolve_hosts(&self, instance: Option<&str>) -> Vec<String> {
        let host = self
            .effective_instance(instance)
            .map(|instance| instance.split('/').next().unwrap_or("").to_string())
            .filter(|host| !host.is_empty());
        self.internal_hosts
            .iter()
            .filter_map(
                |pattern| match (&host, pattern.contains(INSTANCE_PLACEHOLDER)) {
                    (_, false) => Some(pattern.clone()),
                    (Some(host), true) => Some(pattern.replace(INSTANCE_PLACEHOLDER, host)),
                    (None, true) => None,
                },
            )
            .collect()
    }

    /// Resolves the service's `user_agent` preset for `platform` (see
    /// [`resolve_user_agent_preset`]).
    pub fn resolve_user_agent(&self, platform: &str) -> Option<String> {
        resolve_user_agent_preset(self.user_agent.as_deref(), platform)
    }

    /// Converts a user-defined service (see [`normalize_custom`]) into a definition:
    /// internal host = the URL's host (may contain `{instance}`), unread from the title
    /// ([`DEFAULT_TITLE_REGEX`] unless set), user agent "webkit" unless set.
    pub fn from_custom(service: &CustomService) -> Result<ServiceDef, String> {
        let custom = normalize_custom(service)?;
        if custom.id.is_empty() {
            return Err(format!("custom service {:?}: empty id", custom.name));
        }
        let host = url_host_pattern(&custom.url);
        Ok(ServiceDef {
            id: custom.id,
            name: custom.name,
            color: custom.color,
            url: custom.url,
            instance_label: custom.instance_label,
            instance_default: None,
            user_agent: Some(custom.user_agent.unwrap_or_else(|| "webkit".into())),
            internal_hosts: host.into_iter().collect(),
            allow_media: false,
            clear_local_storage: Vec::new(),
            unread: UnreadRules {
                title_regex: Some(
                    custom
                        .title_regex
                        .unwrap_or_else(|| DEFAULT_TITLE_REGEX.into()),
                ),
                title_indirect_regex: None,
                poll: None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{PLATFORM, def};
    use super::super::user_agent::chrome_user_agent;
    use super::*;

    #[test]
    fn instance_substitution() {
        let d = def("https://{instance}/", Some("mastodon.social"));
        assert_eq!(
            d.resolve_url(None).unwrap().as_str(),
            "https://mastodon.social/"
        );
        assert_eq!(
            d.resolve_url(Some("fosstodon.org")).unwrap().host_str(),
            Some("fosstodon.org")
        );
        assert_eq!(
            d.resolve_url(Some("  https://chat.example.org/ "))
                .unwrap()
                .as_str(),
            "https://chat.example.org/"
        );
    }

    #[test]
    fn missing_instance_and_bad_url() {
        let d = def("https://{instance}/", None);
        assert_eq!(
            d.resolve_url(None),
            Err(CatalogError::MissingInstance("t".into()))
        );
        let d = def("ftp://x/", None);
        assert!(matches!(
            d.resolve_url(None),
            Err(CatalogError::InvalidUrl { .. })
        ));
    }

    #[test]
    fn hosts_and_user_agent() {
        let mut d = def("https://{instance}/", None);
        d.internal_hosts = vec!["{instance}".into(), "*.a.io".into()];
        assert_eq!(
            d.resolve_hosts(Some("https://h.example.org/path")),
            vec!["h.example.org", "*.a.io"]
        );
        assert_eq!(d.resolve_hosts(None), vec!["*.a.io"]);
        assert_eq!(d.resolve_user_agent(PLATFORM), None);
        d.user_agent = Some("chrome".into());
        assert_eq!(
            d.resolve_user_agent(PLATFORM),
            Some(chrome_user_agent(PLATFORM))
        );
        d.user_agent = Some("Foo/1".into());
        assert_eq!(d.resolve_user_agent(PLATFORM).as_deref(), Some("Foo/1"));
    }
}
