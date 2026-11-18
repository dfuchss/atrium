//! Service definitions (own TOML format, see `services/*.toml` in the repo root).
//!
//! Example:
//! ```toml
//! id = "mastodon"
//! name = "Mastodon"
//! color = "#6364FF"
//! url = "https://{instance}/"            # {instance} is replaced by the account's instance value
//! instance_label = "Instance"            # optional: ask for an instance value when adding
//! instance_default = "mastodon.social"   # optional
//! user_agent = "webkit"              # optional: "webkit" | "chrome" | "default" | literal UA
//! internal_hosts = ["{instance}"]        # `*.` wildcard allowed, {instance} substituted
//! allow_media = true                # optional: grant camera/mic/screen capture (default false)
//!
//! [unread]
//! title_regex = '^\((\d+)\)'          # capture group 1 = direct unread count
//! title_indirect_regex = '^\*'        # match => indirect unread (dot)
//! poll = "document.querySelectorAll('.unread').length"  # optional JS expression (number or {direct, indirect})
//! ```

mod custom;
mod hosts;
mod parse;
mod service;
mod user_agent;

pub(crate) use custom::{color_or_default, custom_service_id, normalize_custom};
pub use hosts::host_matches_any;
pub(crate) use hosts::normalize_app_hosts;
pub use service::ServiceDef;
pub(crate) use service::normalized_instance;
pub(crate) use user_agent::resolve_user_agent_preset;
pub use user_agent::webkit_user_agent;

/// Built-in service definitions, embedded at compile time.
const BUILTIN_TOML: &[(&str, &str)] = &[
    ("element", include_str!("../../../../services/element.toml")),
    ("slack", include_str!("../../../../services/slack.toml")),
    (
        "linkedin",
        include_str!("../../../../services/linkedin.toml"),
    ),
    (
        "mastodon",
        include_str!("../../../../services/mastodon.toml"),
    ),
    ("discord", include_str!("../../../../services/discord.toml")),
    ("rainbow", include_str!("../../../../services/rainbow.toml")),
    (
        "whatsapp",
        include_str!("../../../../services/whatsapp.toml"),
    ),
    ("teams", include_str!("../../../../services/teams.toml")),
    ("bluesky", include_str!("../../../../services/bluesky.toml")),
];

#[derive(Debug, Clone, PartialEq)]
pub enum CatalogError {
    Parse { source: String, message: String },
    InvalidRegex { service: String, message: String },
    MissingInstance(String),
    InvalidUrl { service: String, url: String },
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::Parse { source, message } => write!(formatter, "{source}: {message}"),
            CatalogError::InvalidRegex { service, message } => {
                write!(
                    formatter,
                    "{service}: invalid regular expression: {message}"
                )
            }
            CatalogError::MissingInstance(service) => {
                write!(formatter, "service {service} needs an instance value")
            }
            CatalogError::InvalidUrl { service, url } => {
                write!(
                    formatter,
                    "service {service}: invalid URL {url:?} (http/https expected)"
                )
            }
        }
    }
}

impl std::error::Error for CatalogError {}

/// All known services; user definitions override built-ins with the same id.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    services: Vec<ServiceDef>,
}

impl Catalog {
    /// Parses [`BUILTIN_TOML`]. Panics only if a built-in file is invalid (covered by tests).
    pub fn builtin() -> Catalog {
        let mut catalog = Catalog::default();
        for (name, text) in BUILTIN_TOML {
            match Catalog::parse(name, text) {
                Ok(def) => catalog.insert(def),
                Err(error) => panic!("invalid built-in service {name}: {error}"),
            }
        }
        catalog
    }

    /// Adds or replaces (same id) a definition.
    pub fn insert(&mut self, def: ServiceDef) {
        match self
            .services
            .iter_mut()
            .find(|service| service.id == def.id)
        {
            Some(existing) => *existing = def,
            None => self.services.push(def),
        }
    }

    pub fn get(&self, id: &str) -> Option<&ServiceDef> {
        self.services.iter().find(|service| service.id == id)
    }

    /// In insertion order (built-ins first, in [`BUILTIN_TOML`] order).
    pub fn services(&self) -> &[ServiceDef] {
        &self.services
    }
}

#[cfg(test)]
mod tests {
    use protocol::CustomService;

    use super::*;

    pub(super) const PLATFORM: &str = "X11; Linux aarch64";

    pub(super) fn def(url: &str, default: Option<&str>) -> ServiceDef {
        let mut d = Catalog::parse("t", "id='t'\nname='T'\ncolor='#000'\nurl='x'").unwrap();
        d.url = url.into();
        d.instance_default = default.map(String::from);
        d
    }

    #[test]
    fn builtins_parse() {
        let c = Catalog::builtin();
        assert_eq!(c.services().len(), BUILTIN_TOML.len());
        assert_eq!(c.services()[0].id, "element");
        assert!(c.get("slack").is_some());
        for (id, media) in [
            ("slack", true),
            ("discord", true),
            ("element", true),
            ("rainbow", true),
            ("linkedin", false),
            ("mastodon", false),
        ] {
            assert_eq!(c.get(id).unwrap().allow_media, media, "{id}");
        }
    }

    #[test]
    fn errors_read_as_sentences() {
        let error = Catalog::parse("f.toml", "id=''\nname='a'\ncolor='#000'\nurl='u'").unwrap_err();
        assert_eq!(error.to_string(), "f.toml: empty id");
        let bad = "id='a'\nname='a'\ncolor='#000'\nurl='u'\n[unread]\ntitle_regex='('";
        let error = Catalog::parse("f.toml", bad).unwrap_err().to_string();
        assert!(
            error.starts_with("f.toml: invalid regular expression: "),
            "{error}"
        );
        let missing = def("https://{instance}/", None)
            .resolve_url(None)
            .unwrap_err();
        assert_eq!(missing.to_string(), "service t needs an instance value");
        let invalid = def("ftp://x/", None).resolve_url(None).unwrap_err();
        assert_eq!(
            invalid.to_string(),
            r#"service t: invalid URL "ftp://x/" (http/https expected)"#
        );
    }

    #[test]
    fn insert_replaces_in_place() {
        let mut c = Catalog::builtin();
        let mut d = c.get("slack").unwrap().clone();
        d.name = "Other".into();
        c.insert(d);
        assert_eq!(c.services().len(), BUILTIN_TOML.len());
        assert_eq!(c.services()[1].name, "Other");
    }

    pub(super) fn custom(name: &str, url: &str) -> CustomService {
        CustomService {
            id: "custom-x".into(),
            name: name.into(),
            url: url.into(),
            color: " ".into(),
            instance_label: None,
            title_regex: None,
            user_agent: None,
        }
    }
}
