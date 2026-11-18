//! Host names and `*.domain` patterns: the app host list and host matching.

/// Normalises the "always open in the app" host list: trim, lowercase, strip scheme, credentials,
/// port and path of pasted URLs, drop empty entries and duplicates. Returns the valid patterns
/// and the rejected raw entries (not a plausible host or `*.host` pattern).
pub(crate) fn normalize_app_hosts(raw: &[String]) -> (Vec<String>, Vec<String>) {
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

/// The lowercase host of a pasted entry: without scheme, path, query, credentials and a
/// numeric port.
fn host_of_entry(entry: &str) -> String {
    let authority = authority_host(entry);
    match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|character| character.is_ascii_digit()) => {
            host.to_string()
        }
        _ => authority,
    }
}

/// Host part of a URL template (see [`authority_host`]), without anything from the first `:`.
pub(super) fn url_host_pattern(url: &str) -> Option<String> {
    let authority = authority_host(url);
    let host = authority.split(':').next().unwrap_or("");
    (!host.is_empty()).then(|| host.to_string())
}

/// The lowercase host part of a URL or pasted entry, textually (so `{instance}` survives):
/// without scheme, path, query, fragment and credentials, but with the port if any.
fn authority_host(url: &str) -> String {
    let url = url.trim();
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    host.to_ascii_lowercase()
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

/// True if `host` matches one of `patterns` (exact host or `*.domain`, which also matches the
/// domain itself; case-insensitive).
pub fn host_matches_any(patterns: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    patterns.iter().any(|pattern| matches_host(pattern, &host))
}

fn matches_host(pattern: &str, host: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    match pattern.strip_prefix("*.") {
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => host == pattern,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn host_extraction_differs_only_in_the_port() {
        let url = " HTTPS://u:pw@Chat.IO:8443/x?y#z ";
        assert_eq!(host_of_entry(url), "chat.io");
        assert_eq!(url_host_pattern(url).as_deref(), Some("chat.io"));
        assert_eq!(host_of_entry("chat.io:abc"), "chat.io:abc");
        assert_eq!(url_host_pattern("chat.io:abc").as_deref(), Some("chat.io"));
        assert_eq!(
            url_host_pattern("https://{instance}.chat.io/").as_deref(),
            Some("{instance}.chat.io")
        );
        assert_eq!(url_host_pattern("https:///path"), None);
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
