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

use protocol::{CustomService, ServiceProblem};
use serde::Deserialize;

/// Safari-style UA for `platform`, the host's part in parentheses (e.g.
/// `Macintosh; Intel Mac OS X 10_15_7` or `X11; Linux aarch64`). Sites sniff the platform and the
/// "Version/... Safari/..." tokens, which the engine's bare UA lacks, so some reject it.
pub fn webkit_user_agent(platform: &str) -> String {
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.1 Safari/605.1.15"
    )
}

/// Desktop Chrome UA for `platform` (see [`webkit_user_agent`]), for sites that only accept
/// Chromium-based browsers.
pub fn chrome_user_agent(platform: &str) -> String {
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
    )
}

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
    ("whatsapp", include_str!("../../../services/whatsapp.toml")),
    ("teams", include_str!("../../../services/teams.toml")),
    ("bluesky", include_str!("../../../services/bluesky.toml")),
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
    UnknownService(String),
    MissingInstance(String),
    InvalidUrl { service: String, url: String },
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
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
pub fn line_and_column(text: &str, offset: usize) -> (u32, u32) {
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

/// URL placeholder for the per-account part (instance host, workspace subdomain...).
pub const PLACEHOLDER: &str = "{instance}";

/// Trims an instance value and strips a pasted scheme and trailing slashes.
pub(crate) fn normalize_instance(instance: &str) -> String {
    let trimmed = instance.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    without_scheme.trim_end_matches('/').to_string()
}

/// Normalises the "always open in the app" host list: trim, lowercase, strip scheme, credentials,
/// port and path of pasted URLs, drop empty entries and duplicates. Returns the valid patterns
/// and the rejected raw entries (not a plausible host or `*.host` pattern).
pub fn normalize_app_hosts(raw: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut hosts, mut rejected) = (Vec::<String>::new(), Vec::new());
    for entry in raw {
        let host = host_of_entry(entry);
        if host.is_empty() {
            // A blank line is dropped silently, a URL without host is reported.
            if !entry.trim().is_empty() && entry.contains("://") {
                rejected.push(entry.trim().to_string());
            }
        } else if !is_plausible_host_pattern(&host) {
            rejected.push(entry.trim().to_string());
        } else if !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    (hosts, rejected)
}

/// The lowercase host of a pasted entry: without scheme, path, query, credentials and port.
fn host_of_entry(entry: &str) -> String {
    let mut host = entry.trim().to_ascii_lowercase();
    if let Some((_, rest)) = host.split_once("://") {
        host = rest.to_string();
    }
    let end = host.find(['/', '?', '#']).unwrap_or(host.len());
    host.truncate(end);
    if let Some((_, without_credentials)) = host.rsplit_once('@') {
        host = without_credentials.to_string();
    }
    if let Some((without_port, port)) = host.rsplit_once(':')
        && port.chars().all(|character| character.is_ascii_digit())
    {
        host = without_port.to_string();
    }
    host
}

/// A host name or `*.host` pattern: labels of `[a-z0-9-]`, no empty labels or edge dashes.
fn is_plausible_host_pattern(pattern: &str) -> bool {
    let base = pattern.strip_prefix("*.").unwrap_or(pattern);
    !base.is_empty()
        && !base.starts_with(['.', '-'])
        && !base.ends_with(['.', '-'])
        && !base.contains("..")
        && base.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '.'
        })
}

fn matches_host(pattern: &str, host: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    match pattern.strip_prefix("*.") {
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => host == pattern,
    }
}

impl ServiceDef {
    fn effective_instance(&self, instance: Option<&str>) -> Option<String> {
        instance
            .map(normalize_instance)
            .filter(|instance| !instance.is_empty())
            .or_else(|| {
                self.instance_default
                    .as_deref()
                    .map(normalize_instance)
                    .filter(|instance| !instance.is_empty())
            })
    }

    /// URL with `{instance}` replaced. Errors if the URL needs an instance and none (and no default) is given,
    /// or if the result is not a valid http(s) URL.
    pub fn resolve_url(&self, instance: Option<&str>) -> Result<url::Url, CatalogError> {
        let url = self.url.clone();
        let raw = if url.contains(PLACEHOLDER) {
            let instance = self
                .effective_instance(instance)
                .ok_or_else(|| CatalogError::MissingInstance(self.id.clone()))?;
            url.replace(PLACEHOLDER, &instance)
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
            .filter_map(|pattern| match (&host, pattern.contains(PLACEHOLDER)) {
                (_, false) => Some(pattern.clone()),
                (Some(host), true) => Some(pattern.replace(PLACEHOLDER, host)),
                (None, true) => None,
            })
            .collect()
    }

    /// Resolves the service's `user_agent` preset for `platform` (see
    /// [`resolve_user_agent_preset`]).
    pub fn resolve_user_agent(&self, platform: &str) -> Option<String> {
        resolve_user_agent_preset(self.user_agent.as_deref(), platform)
    }
}

/// Validates and normalizes a user-defined service (trimmed fields, `instance_label` only when the
/// URL has `{instance}`). The URL must be http(s) once `{instance}` is replaced by "example"; the title
/// regex must compile. The id is kept as is.
pub fn normalize_custom(service: &CustomService) -> Result<CustomService, String> {
    let name = service.name.trim().to_string();
    if name.is_empty() {
        return Err("custom service: empty name".into());
    }
    let url = service.url.trim().to_string();
    if !is_web_url_template(&url) {
        return Err(format!(
            "custom service {name:?}: invalid URL {url:?} (http/https expected)"
        ));
    }
    let title_regex = trimmed_option(service.title_regex.as_deref());
    if let Some(pattern) = &title_regex {
        regex::Regex::new(pattern)
            .map_err(|error| format!("custom service {name:?}: invalid title regex: {error}"))?;
    }
    let instance_label = url.contains(PLACEHOLDER).then(|| {
        trimmed_option(service.instance_label.as_deref()).unwrap_or_else(|| "Instance".into())
    });
    let color = color_or_default(&service.color);
    Ok(CustomService {
        id: service.id.trim().to_string(),
        name,
        url,
        color,
        instance_label,
        title_regex,
        user_agent: trimmed_option(service.user_agent.as_deref()),
    })
}

/// True for a hex color `#` + 3 to 8 hex digits. The UI puts service colors into a style
/// attribute, so nothing else may get through.
pub fn is_hex_color(color: &str) -> bool {
    color.strip_prefix('#').is_some_and(|digits| {
        (3..=8).contains(&digits.len()) && digits.bytes().all(|digit| digit.is_ascii_hexdigit())
    })
}

/// The trimmed `color` if it is a hex color (see [`is_hex_color`]), else [`DEFAULT_CUSTOM_COLOR`].
pub fn color_or_default(color: &str) -> String {
    let color = color.trim();
    if is_hex_color(color) {
        color.to_string()
    } else {
        DEFAULT_CUSTOM_COLOR.to_string()
    }
}

/// Trimmed text; empty means none.
fn trimmed_option(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(String::from)
}

/// True if the URL is http(s) with a host once `{instance}` is replaced by "example".
fn is_web_url_template(url: &str) -> bool {
    let probe = url.replace(PLACEHOLDER, "example");
    url::Url::parse(&probe).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
    })
}

impl ServiceDef {
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

/// Host part of a URL template, textually (so `{instance}` survives): no userinfo, no port.
fn url_host_pattern(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host.split(':').next().unwrap_or("").to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// `custom-<slug of name>`, unique among `taken`.
pub fn custom_service_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut slug = String::new();
    for character in name.trim().to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
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
    let mut suffix = 2u32;
    loop {
        let id = format!("{base}-{suffix}");
        if !taken(&id) {
            return id;
        }
        suffix += 1;
    }
}

/// Resolves a user agent preset: "webkit"/"chrome" -> that UA for the host's `platform` (see
/// [`webkit_user_agent`]), "default"/None -> None (the engine's own UA), anything else -> literal.
pub fn resolve_user_agent_preset(preset: Option<&str>, platform: &str) -> Option<String> {
    match preset {
        None | Some("default") => None,
        Some("webkit") => Some(webkit_user_agent(platform)),
        Some("chrome") => Some(chrome_user_agent(platform)),
        Some(other) => Some(other.to_string()),
    }
}

/// True if `host` matches one of `patterns` (exact host or `*.domain`, which also matches the
/// domain itself; case-insensitive).
pub fn host_matches_any(patterns: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    patterns.iter().any(|pattern| matches_host(pattern, &host))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLATFORM: &str = "X11; Linux aarch64";

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

    #[test]
    fn user_agents_carry_the_hosts_platform() {
        assert_eq!(
            webkit_user_agent(PLATFORM),
            "Mozilla/5.0 (X11; Linux aarch64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.1 Safari/605.1.15"
        );
        assert!(chrome_user_agent(PLATFORM).starts_with("Mozilla/5.0 (X11; Linux aarch64) "));
    }

    fn custom(name: &str, url: &str) -> CustomService {
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

    #[test]
    fn custom_service_conversion() {
        let d = ServiceDef::from_custom(&custom(
            " Chat ",
            " https://user@{instance}.chat.io:8443/app?x=1 ",
        ))
        .unwrap();
        assert_eq!(
            (d.id.as_str(), d.name.as_str(), d.color.as_str()),
            ("custom-x", "Chat", DEFAULT_CUSTOM_COLOR)
        );
        assert_eq!(d.internal_hosts, vec!["{instance}.chat.io"]);
        assert_eq!(d.instance_label.as_deref(), Some("Instance"));
        assert_eq!(d.unread.title_regex.as_deref(), Some(DEFAULT_TITLE_REGEX));
        assert_eq!(
            d.resolve_user_agent(PLATFORM),
            Some(webkit_user_agent(PLATFORM))
        );
        assert!(!d.allow_media, "custom services default to no media");
        assert_eq!(d.resolve_hosts(Some("kit")), vec!["kit.chat.io"]);
        assert_eq!(
            d.resolve_url(Some("kit")).unwrap().as_str(),
            "https://user@kit.chat.io:8443/app?x=1"
        );
        let mut c = custom("Plain", "http://Example.org");
        c.instance_label = Some("Ignored".into());
        c.title_regex = Some(r"^\[(\d+)\]".into());
        c.user_agent = Some("chrome".into());
        let d = ServiceDef::from_custom(&c).unwrap();
        assert_eq!(
            (&d.instance_label, &d.internal_hosts),
            (&None, &vec!["example.org".to_string()])
        );
        assert_eq!(d.unread.title_regex.as_deref(), Some(r"^\[(\d+)\]"));
        assert_eq!(
            d.resolve_user_agent(PLATFORM),
            Some(chrome_user_agent(PLATFORM))
        );
    }

    #[test]
    fn custom_service_validation() {
        assert!(normalize_custom(&custom("  ", "https://a.io")).is_err());
        assert!(normalize_custom(&custom("A", "ftp://a.io")).is_err());
        assert!(normalize_custom(&custom("A", "not a url")).is_err());
        assert!(normalize_custom(&custom("A", "https://{instance}/")).is_ok());
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
    fn app_host_normalisation() {
        let (hosts, rejected) = normalize_app_hosts(&[
            "  IdP.Example.org ".into(),
            "https://user:pw@login.example.com:8443/path?q=1".into(),
            "*.MicrosoftOnline.com".into(),
            "idp.example.org".into(),
            String::new(),
            "   ".into(),
            "bad host".into(),
            "a*b.com".into(),
            "**.x.com".into(),
            "*.".into(),
            "http://".into(),
        ]);
        assert_eq!(
            hosts,
            vec![
                "idp.example.org",
                "login.example.com",
                "*.microsoftonline.com"
            ]
        );
        assert_eq!(rejected.len(), 5, "{rejected:?}");
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

    #[test]
    fn colors_must_be_hex() {
        for good in ["#000", "#4A154B", "#0dbd8b80"] {
            assert!(is_hex_color(good), "{good}");
        }
        for bad in [
            "",
            "#",
            "#12",
            "#123456789",
            "red",
            "#zzz",
            "#000;background:url(x)",
            "000",
        ] {
            assert!(!is_hex_color(bad), "{bad}");
        }
        let toml =
            "id='a'\nname='A'\ncolor='red;background:url(https://x.org)'\nurl='https://a.io'";
        assert!(matches!(
            Catalog::parse("s", toml),
            Err(CatalogError::Parse { message, .. }) if message.contains("invalid color")
        ));
        let mut service = custom("A", "https://a.io");
        service.color = " #ABC ".into();
        assert_eq!(normalize_custom(&service).unwrap().color, "#ABC");
        service.color = "x\" onclick=\"y".into();
        assert_eq!(
            normalize_custom(&service).unwrap().color,
            DEFAULT_CUSTOM_COLOR
        );
    }
}
