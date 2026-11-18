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
mod user_agent;

use protocol::{CustomService, INSTANCE_PLACEHOLDER, ServiceProblem};
use serde::Deserialize;

use custom::DEFAULT_TITLE_REGEX;
use hosts::url_host_pattern;

pub(crate) use custom::{color_or_default, custom_service_id, normalize_custom};
pub use hosts::host_matches_any;
pub(crate) use hosts::normalize_app_hosts;
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

/// [`Catalog::parse`] that also returns the byte range of a TOML syntax or type error.
fn parse_located(
    source: &str,
    toml_text: &str,
) -> Result<ServiceDef, (CatalogError, Option<std::ops::Range<usize>>)> {
    let parse_err = |message: String| CatalogError::Parse {
        source: source.to_string(),
        message,
    };
    let def: ServiceDef = toml::from_str(toml_text)
        .map_err(|error| (parse_err(error.message().to_string()), error.span()))?;
    if def.id.trim().is_empty() {
        return Err((parse_err("empty id".into()), None));
    }
    if def.name.trim().is_empty() {
        return Err((parse_err("empty name".into()), None));
    }
    if !is_hex_color(&def.color) {
        let message = format!("invalid color {:?} (#rgb or #rrggbb expected)", def.color);
        return Err((parse_err(message), None));
    }
    let rules = [&def.unread.title_regex, &def.unread.title_indirect_regex];
    for pattern in rules.into_iter().flatten() {
        regex::Regex::new(pattern).map_err(|error| {
            let invalid = CatalogError::InvalidRegex {
                service: source.to_string(),
                message: error.to_string(),
            };
            (invalid, None)
        })?;
    }
    Ok(def)
}

/// 1-based line and column (in characters) of the byte `offset` in `text`; an offset past the
/// end or inside a character is clamped to the nearest earlier boundary.
fn line_and_column(text: &str, offset: usize) -> (u32, u32) {
    let mut end = offset.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let column = before[line_start..].chars().count() + 1;
    (
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(column).unwrap_or(u32::MAX),
    )
}

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

    /// Parses one TOML definition and validates its regexes. `source` names it in errors.
    pub fn parse(source: &str, toml_text: &str) -> Result<ServiceDef, CatalogError> {
        parse_located(source, toml_text).map_err(|(error, span)| match (error, span) {
            (CatalogError::Parse { source, message }, Some(span)) => {
                let (line, column) = line_and_column(toml_text, span.start);
                CatalogError::Parse {
                    source,
                    message: format!("{message} (line {line}, column {column})"),
                }
            }
            (error, _) => error,
        })
    }

    /// The problems of a definition text as the editor shows them: the same checks as
    /// [`Catalog::parse`], with line and column where the TOML parser knows a position.
    pub fn diagnose(toml_text: &str) -> Vec<ServiceProblem> {
        let Err((error, span)) = parse_located("", toml_text) else {
            return vec![];
        };
        let message = match error {
            CatalogError::Parse { message, .. } => message,
            CatalogError::InvalidRegex { message, .. } => {
                format!("invalid regular expression: {message}")
            }
            other => other.to_string(),
        };
        let position = span.map(|span| line_and_column(toml_text, span.start));
        vec![ServiceProblem {
            message,
            line: position.map(|(line, _)| line),
            column: position.map(|(_, column)| column),
        }]
    }

    /// Parses the user's definition files, given as `(source, text)` in the order they are
    /// applied (later files win). Invalid files are skipped and reported as messages.
    pub fn parse_user_files(files: &[(String, String)]) -> (Vec<ServiceDef>, Vec<String>) {
        let (mut defs, mut errors) = (vec![], vec![]);
        for (source, text) in files {
            match Catalog::parse(source, text) {
                Ok(def) => defs.push(def),
                Err(error) => errors.push(format!("invalid service definition: {error}")),
            }
        }
        (defs, errors)
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

/// True for a hex color `#` + 3 to 8 hex digits. The UI puts service colors into a style
/// attribute, so nothing else may get through.
fn is_hex_color(color: &str) -> bool {
    color.strip_prefix('#').is_some_and(|digits| {
        (3..=8).contains(&digits.len()) && digits.bytes().all(|digit| digit.is_ascii_hexdigit())
    })
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
    use super::user_agent::chrome_user_agent;
    use super::*;

    pub(super) const PLATFORM: &str = "X11; Linux aarch64";

    fn def(url: &str, default: Option<&str>) -> ServiceDef {
        let mut d = Catalog::parse("t", "id='t'\nname='T'\ncolor='#000'\nurl='x'").unwrap();
        d.url = url.into();
        d.instance_default = default.map(String::from);
        d
    }

    #[test]
    fn line_and_column_counts_characters_from_one() {
        let text = "ab\nc\u{e4}d\n";
        assert_eq!(line_and_column(text, 0), (1, 1));
        assert_eq!(line_and_column(text, 2), (1, 3));
        assert_eq!(line_and_column(text, 3), (2, 1));
        assert_eq!(line_and_column(text, 6), (2, 3));
        assert_eq!(line_and_column(text, 8), (3, 1));
        assert_eq!(line_and_column(text, 400), (3, 1));
        // Offset 5 is inside the two-byte "\u{e4}": clamped to its start.
        assert_eq!(line_and_column(text, 5), (2, 2));
    }

    #[test]
    fn diagnose_accepts_a_valid_definition() {
        assert!(
            Catalog::diagnose("id='a'\nname='A'\ncolor='#111111'\nurl='https://a/'").is_empty()
        );
    }

    #[test]
    fn diagnose_locates_syntax_errors() {
        let problems = Catalog::diagnose("id = 'a'\nname = \n");
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].line, Some(2));
        assert!(problems[0].column.is_some());
        assert!(
            !problems[0].message.contains('\n'),
            "{:?}",
            problems[0].message
        );
    }

    #[test]
    fn diagnose_reports_missing_fields_and_bad_regexes() {
        let missing = Catalog::diagnose("id = 'a'\nname = 'A'\n");
        assert!(missing[0].message.contains("url") || missing[0].message.contains("color"));
        let regex =
            Catalog::diagnose("id='a'\nname='A'\ncolor='#111'\nurl='x'\n[unread]\ntitle_regex='('");
        assert!(regex[0].message.starts_with("invalid regular expression"));
        assert_eq!(regex[0].line, None);
        let empty_id = Catalog::diagnose("id=' '\nname='A'\ncolor='#111'\nurl='x'");
        assert_eq!(empty_id[0].message, "empty id");
    }

    #[test]
    fn parse_errors_in_logs_name_the_position() {
        let error = Catalog::parse("f.toml", "id = 'a'\nname = ").unwrap_err();
        assert!(error.to_string().contains("line 2"), "{error}");
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
    fn parse_errors() {
        assert!(matches!(
            Catalog::parse("s", "garbage ="),
            Err(CatalogError::Parse { .. })
        ));
        assert!(matches!(
            Catalog::parse("s", "id=''\nname='a'\ncolor='#000'\nurl='u'"),
            Err(CatalogError::Parse { .. })
        ));
        let bad = "id='a'\nname='a'\ncolor='#000'\nurl='u'\n[unread]\ntitle_regex='('";
        match Catalog::parse("src", bad) {
            Err(CatalogError::InvalidRegex { service, .. }) => assert_eq!(service, "src"),
            other => panic!("{other:?}"),
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
