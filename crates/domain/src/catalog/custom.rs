//! User-defined services: validation, colors and ids.

use protocol::{CustomService, INSTANCE_PLACEHOLDER};

use super::parse::is_hex_color;

/// Unread rule of custom services without an own `title_regex`: "(3) Title".
pub(super) const DEFAULT_TITLE_REGEX: &str = r"^\((\d+)\)";
/// Sidebar color of custom services without an own color.
const DEFAULT_CUSTOM_COLOR: &str = "#607D8B";
/// Id prefix of user-defined services.
const CUSTOM_PREFIX: &str = "custom-";

/// Validates and normalizes a user-defined service (trimmed fields, `instance_label` only when the
/// URL has `{instance}`). The URL must be http(s) once `{instance}` is replaced by "example"; the title
/// regex must compile. The id is kept as is.
pub(crate) fn normalize_custom(service: &CustomService) -> Result<CustomService, String> {
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
    let title_regex = crate::trimmed(service.title_regex.as_deref());
    if let Some(pattern) = &title_regex {
        regex::Regex::new(pattern)
            .map_err(|error| format!("custom service {name:?}: invalid title regex: {error}"))?;
    }
    let instance_label = url.contains(INSTANCE_PLACEHOLDER).then(|| {
        crate::trimmed(service.instance_label.as_deref()).unwrap_or_else(|| "Instance".into())
    });
    let color = color_or_default(&service.color);
    Ok(CustomService {
        id: service.id.trim().to_string(),
        name,
        url,
        color,
        instance_label,
        title_regex,
        user_agent: crate::trimmed(service.user_agent.as_deref()),
    })
}

/// The trimmed `color` if it is a hex color (see [`is_hex_color`]), else [`DEFAULT_CUSTOM_COLOR`].
pub(crate) fn color_or_default(color: &str) -> String {
    let color = color.trim();
    if is_hex_color(color) {
        color.to_string()
    } else {
        DEFAULT_CUSTOM_COLOR.to_string()
    }
}

/// True if the URL is http(s) with a host once `{instance}` is replaced by "example".
fn is_web_url_template(url: &str) -> bool {
    let probe = url.replace(INSTANCE_PLACEHOLDER, "example");
    url::Url::parse(&probe).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
    })
}

/// `custom-<slug of name>`, unique among `taken`.
pub(crate) fn custom_service_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
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

#[cfg(test)]
mod tests {
    use super::super::tests::{PLATFORM, custom};
    use super::super::user_agent::{chrome_user_agent, webkit_user_agent};
    use super::super::{Catalog, CatalogError, ServiceDef};
    use super::*;

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
