//! Service definitions (own TOML format, see `services/*.toml` in the repo root).
//!
//! Example:
//! ```toml
//! id = "mastodon"
//! name = "Mastodon"
//! color = "#6364FF"
//! url = "https://{team}/"            # {team} is replaced by the account's team value
//! team_label = "Instance"            # optional: ask for a team value when adding
//! team_default = "mastodon.social"   # optional
//! user_agent = "safari"              # optional: "safari" | "chrome" | "default" | literal UA
//! internal_hosts = ["{team}"]        # `*.` wildcard allowed, {team} substituted
//! allow_media = true                # optional: grant camera/mic/screen capture (default false)
//!
//! [unread]
//! title_regex = '^\((\d+)\)'          # capture group 1 = direct unread count
//! title_indirect_regex = '^\*'        # match => indirect unread (dot)
//! poll = "document.querySelectorAll('.unread').length"  # optional JS expression (number or {direct, indirect})
//! ```

use protocol::CustomService;
use serde::Deserialize;

/// Desktop Safari UA; WKWebView's default UA lacks "Version/... Safari/..." and some sites reject it.
pub const UA_SAFARI: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.1 Safari/605.1.15";
pub const UA_CHROME: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";

/// Unread rule of custom services without an own `title_regex`: "(3) Title".
pub const DEFAULT_TITLE_REGEX: &str = r"^\((\d+)\)";
/// Sidebar color of custom services without an own color.
pub const DEFAULT_CUSTOM_COLOR: &str = "#607D8B";
/// Id prefix of user-defined services.
pub const CUSTOM_PREFIX: &str = "custom-";

/// Built-in service definitions, embedded at compile time.
pub const BUILTIN_TOML: &[(&str, &str)] = &[
    ("element", include_str!("../../../services/element.toml")),
    ("slack", include_str!("../../../services/slack.toml")),
    ("linkedin", include_str!("../../../services/linkedin.toml")),
    ("mastodon", include_str!("../../../services/mastodon.toml")),
    ("discord", include_str!("../../../services/discord.toml")),
    ("rainbow", include_str!("../../../services/rainbow.toml")),
];

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ServiceDef {
    pub id: String,
    pub name: String,
    pub color: String,
    pub url: String,
    #[serde(default)]
    pub team_label: Option<String>,
    #[serde(default)]
    pub team_default: Option<String>,
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
    UnknownService(String),
    MissingTeam(String),
    InvalidUrl { service: String, url: String },
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
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
                Err(e) => panic!("invalid built-in service {name}: {e}"),
            }
        }
        catalog
    }

    /// Parses one TOML definition and validates its regexes. `source` names it in errors.
    pub fn parse(source: &str, toml_text: &str) -> Result<ServiceDef, CatalogError> {
        let parse_err = |message: String| CatalogError::Parse {
            source: source.to_string(),
            message,
        };
        let def: ServiceDef = toml::from_str(toml_text).map_err(|e| parse_err(e.to_string()))?;
        if def.id.trim().is_empty() {
            return Err(parse_err("empty id".into()));
        }
        if def.name.trim().is_empty() {
            return Err(parse_err("empty name".into()));
        }
        let rules = [&def.unread.title_regex, &def.unread.title_indirect_regex];
        for pattern in rules.into_iter().flatten() {
            regex::Regex::new(pattern).map_err(|e| CatalogError::InvalidRegex {
                service: source.to_string(),
                message: e.to_string(),
            })?;
        }
        Ok(def)
    }

    /// Parses the user's definition files, given as `(source, text)` in the order they are
    /// applied (later files win). Invalid files are skipped and reported as messages.
    pub fn parse_user_files(files: &[(String, String)]) -> (Vec<ServiceDef>, Vec<String>) {
        let (mut defs, mut errors) = (vec![], vec![]);
        for (source, text) in files {
            match Catalog::parse(source, text) {
                Ok(def) => defs.push(def),
                Err(e) => errors.push(format!("invalid service definition: {e}")),
            }
        }
        (defs, errors)
    }

    /// Adds or replaces (same id) a definition.
    pub fn insert(&mut self, def: ServiceDef) {
        match self.services.iter_mut().find(|s| s.id == def.id) {
            Some(existing) => *existing = def,
            None => self.services.push(def),
        }
    }

    pub fn get(&self, id: &str) -> Option<&ServiceDef> {
        self.services.iter().find(|s| s.id == id)
    }

    /// In insertion order (built-ins first, in [`BUILTIN_TOML`] order).
    pub fn services(&self) -> &[ServiceDef] {
        &self.services
    }
}

/// Trims a team value and strips a pasted scheme and trailing slashes.
pub(crate) fn normalize_team(team: &str) -> String {
    let t = team.trim();
    let t = t
        .strip_prefix("https://")
        .or_else(|| t.strip_prefix("http://"))
        .unwrap_or(t);
    t.trim_end_matches('/').to_string()
}

fn matches_host(pattern: &str, host: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    match pattern.strip_prefix("*.") {
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => host == pattern,
    }
}

impl ServiceDef {
    fn effective_team(&self, team: Option<&str>) -> Option<String> {
        team.map(normalize_team)
            .filter(|t| !t.is_empty())
            .or_else(|| {
                self.team_default
                    .as_deref()
                    .map(normalize_team)
                    .filter(|t| !t.is_empty())
            })
    }

    /// URL with `{team}` replaced. Errors if the URL needs a team and none (and no default) is given,
    /// or if the result is not a valid http(s) URL.
    pub fn resolve_url(&self, team: Option<&str>) -> Result<url::Url, CatalogError> {
        let raw = if self.url.contains("{team}") {
            let t = self
                .effective_team(team)
                .ok_or_else(|| CatalogError::MissingTeam(self.id.clone()))?;
            self.url.replace("{team}", &t)
        } else {
            self.url.clone()
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

    /// `internal_hosts` with `{team}` replaced (team = host part of the team value).
    pub fn resolve_hosts(&self, team: Option<&str>) -> Vec<String> {
        let host = self
            .effective_team(team)
            .map(|t| t.split('/').next().unwrap_or("").to_string())
            .filter(|h| !h.is_empty());
        self.internal_hosts
            .iter()
            .filter_map(|h| match (&host, h.contains("{team}")) {
                (_, false) => Some(h.clone()),
                (Some(host), true) => Some(h.replace("{team}", host)),
                (None, true) => None,
            })
            .collect()
    }

    /// Resolves the service's `user_agent` preset (see [`resolve_user_agent_preset`]).
    pub fn resolve_user_agent(&self) -> Option<String> {
        resolve_user_agent_preset(self.user_agent.as_deref())
    }
}

/// Validates and normalizes a user-defined service (trimmed fields, `team_label` only when the
/// URL has `{team}`). The URL must be http(s) once `{team}` is replaced by "example"; the title
/// regex must compile. The id is kept as is.
pub fn normalize_custom(c: &CustomService) -> Result<CustomService, String> {
    let opt = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    let name = c.name.trim().to_string();
    if name.is_empty() {
        return Err("custom service: empty name".into());
    }
    let url = c.url.trim().to_string();
    let probe = url.replace("{team}", "example");
    match url::Url::parse(&probe) {
        Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => {}
        _ => {
            return Err(format!(
                "custom service {name:?}: invalid URL {url:?} (http/https expected)"
            ));
        }
    }
    let title_regex = opt(&c.title_regex);
    if let Some(re) = &title_regex {
        regex::Regex::new(re)
            .map_err(|e| format!("custom service {name:?}: invalid title regex: {e}"))?;
    }
    let team_label = if url.contains("{team}") {
        Some(opt(&c.team_label).unwrap_or_else(|| "Team".into()))
    } else {
        None
    };
    let color = Some(c.color.trim())
        .filter(|c| !c.is_empty())
        .unwrap_or(DEFAULT_CUSTOM_COLOR)
        .to_string();
    Ok(CustomService {
        id: c.id.trim().to_string(),
        name,
        url,
        color,
        team_label,
        title_regex,
        user_agent: opt(&c.user_agent),
    })
}

impl ServiceDef {
    /// Converts a user-defined service (see [`normalize_custom`]) into a definition:
    /// internal host = the URL's host (may contain `{team}`), unread from the title
    /// ([`DEFAULT_TITLE_REGEX`] unless set), user agent "safari" unless set.
    pub fn from_custom(c: &CustomService) -> Result<ServiceDef, String> {
        let c = normalize_custom(c)?;
        if c.id.is_empty() {
            return Err(format!("custom service {:?}: empty id", c.name));
        }
        let host = url_host_pattern(&c.url);
        Ok(ServiceDef {
            id: c.id,
            name: c.name,
            color: c.color,
            url: c.url,
            team_label: c.team_label,
            team_default: None,
            user_agent: Some(c.user_agent.unwrap_or_else(|| "safari".into())),
            internal_hosts: host.into_iter().collect(),
            allow_media: false,
            clear_local_storage: Vec::new(),
            unread: UnreadRules {
                title_regex: Some(c.title_regex.unwrap_or_else(|| DEFAULT_TITLE_REGEX.into())),
                title_indirect_regex: None,
                poll: None,
            },
        })
    }
}

/// Host part of a URL template, textually (so `{team}` survives): no userinfo, no port.
fn url_host_pattern(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.split(':').next().unwrap_or("").to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// `custom-<slug of name>`, unique among `taken`.
pub fn custom_service_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut slug = String::new();
    for ch in name.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let base = format!(
        "{CUSTOM_PREFIX}{}",
        if slug.is_empty() { "service" } else { slug }
    );
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken(id))
        .expect("unbounded")
}

/// Resolves a user agent preset: "safari"/"chrome" -> constant, "default"/None -> None
/// (the engine's own UA), anything else -> literal.
pub fn resolve_user_agent_preset(preset: Option<&str>) -> Option<String> {
    match preset {
        None | Some("default") => None,
        Some("safari") => Some(UA_SAFARI.to_string()),
        Some("chrome") => Some(UA_CHROME.to_string()),
        Some(other) => Some(other.to_string()),
    }
}

pub(crate) fn host_matches_any(patterns: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    patterns.iter().any(|p| matches_host(p, &host))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(url: &str, default: Option<&str>) -> ServiceDef {
        let mut d = Catalog::parse("t", "id='t'\nname='T'\ncolor='#000'\nurl='x'").unwrap();
        d.url = url.into();
        d.team_default = default.map(String::from);
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
    fn parse_errors() {
        assert!(matches!(
            Catalog::parse("s", "garbage ="),
            Err(CatalogError::Parse { .. })
        ));
        assert!(matches!(
            Catalog::parse("s", "id=''\nname='a'\ncolor='c'\nurl='u'"),
            Err(CatalogError::Parse { .. })
        ));
        let bad = "id='a'\nname='a'\ncolor='c'\nurl='u'\n[unread]\ntitle_regex='('";
        match Catalog::parse("src", bad) {
            Err(CatalogError::InvalidRegex { service, .. }) => assert_eq!(service, "src"),
            other => panic!("{other:?}"),
        }
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
    fn team_substitution() {
        let d = def("https://{team}/", Some("mastodon.social"));
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
    fn missing_team_and_bad_url() {
        let d = def("https://{team}/", None);
        assert_eq!(
            d.resolve_url(None),
            Err(CatalogError::MissingTeam("t".into()))
        );
        let d = def("ftp://x/", None);
        assert!(matches!(
            d.resolve_url(None),
            Err(CatalogError::InvalidUrl { .. })
        ));
    }

    #[test]
    fn hosts_and_user_agent() {
        let mut d = def("https://{team}/", None);
        d.internal_hosts = vec!["{team}".into(), "*.a.io".into()];
        assert_eq!(
            d.resolve_hosts(Some("https://h.example.org/path")),
            vec!["h.example.org", "*.a.io"]
        );
        assert_eq!(d.resolve_hosts(None), vec!["*.a.io"]);
        assert_eq!(d.resolve_user_agent(), None);
        d.user_agent = Some("chrome".into());
        assert_eq!(d.resolve_user_agent().as_deref(), Some(UA_CHROME));
        d.user_agent = Some("Foo/1".into());
        assert_eq!(d.resolve_user_agent().as_deref(), Some("Foo/1"));
    }

    fn custom(name: &str, url: &str) -> CustomService {
        CustomService {
            id: "custom-x".into(),
            name: name.into(),
            url: url.into(),
            color: " ".into(),
            team_label: None,
            title_regex: None,
            user_agent: None,
        }
    }

    #[test]
    fn custom_service_conversion() {
        let d = ServiceDef::from_custom(&custom(
            " Chat ",
            " https://user@{team}.chat.io:8443/app?x=1 ",
        ))
        .unwrap();
        assert_eq!(
            (d.id.as_str(), d.name.as_str(), d.color.as_str()),
            ("custom-x", "Chat", DEFAULT_CUSTOM_COLOR)
        );
        assert_eq!(d.internal_hosts, vec!["{team}.chat.io"]);
        assert_eq!(d.team_label.as_deref(), Some("Team"));
        assert_eq!(d.unread.title_regex.as_deref(), Some(DEFAULT_TITLE_REGEX));
        assert_eq!(d.resolve_user_agent().as_deref(), Some(UA_SAFARI));
        assert!(!d.allow_media, "custom services default to no media");
        assert_eq!(d.resolve_hosts(Some("kit")), vec!["kit.chat.io"]);
        assert_eq!(
            d.resolve_url(Some("kit")).unwrap().as_str(),
            "https://user@kit.chat.io:8443/app?x=1"
        );
        let mut c = custom("Plain", "http://Example.org");
        c.team_label = Some("Ignored".into());
        c.title_regex = Some(r"^\[(\d+)\]".into());
        c.user_agent = Some("chrome".into());
        let d = ServiceDef::from_custom(&c).unwrap();
        assert_eq!(
            (&d.team_label, &d.internal_hosts),
            (&None, &vec!["example.org".to_string()])
        );
        assert_eq!(d.unread.title_regex.as_deref(), Some(r"^\[(\d+)\]"));
        assert_eq!(d.resolve_user_agent().as_deref(), Some(UA_CHROME));
    }

    #[test]
    fn custom_service_validation() {
        assert!(normalize_custom(&custom("  ", "https://a.io")).is_err());
        assert!(normalize_custom(&custom("A", "ftp://a.io")).is_err());
        assert!(normalize_custom(&custom("A", "not a url")).is_err());
        assert!(normalize_custom(&custom("A", "https://{team}/")).is_ok());
        let mut c = custom("A", "https://a.io");
        c.title_regex = Some("(".into());
        assert!(normalize_custom(&c).unwrap_err().contains("regex"));
        let mut c = custom("A", "https://a.io");
        c.id = String::new();
        assert!(ServiceDef::from_custom(&c).is_err());
    }

    #[test]
    fn custom_ids() {
        assert_eq!(custom_service_id(" My Chat! ", |_| false), "custom-my-chat");
        assert_eq!(custom_service_id("ÄÖ", |_| false), "custom-service");
        let taken = ["custom-a", "custom-a-2"];
        assert_eq!(
            custom_service_id("a", |id| taken.contains(&id)),
            "custom-a-3"
        );
    }

    #[test]
    fn wildcard_matching() {
        let pats = vec!["*.x.com".to_string(), "exact.org".to_string()];
        assert!(host_matches_any(&pats, "x.com"));
        assert!(host_matches_any(&pats, "A.X.com"));
        assert!(!host_matches_any(&pats, "notx.com"));
        assert!(host_matches_any(&pats, "exact.org"));
        assert!(!host_matches_any(&pats, "sub.exact.org"));
    }
}
