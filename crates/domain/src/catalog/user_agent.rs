//! The user agent presets of the service views.

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
pub(super) fn chrome_user_agent(platform: &str) -> String {
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
    )
}

/// Resolves a user agent preset: "webkit"/"chrome" -> that UA for the host's `platform` (see
/// [`webkit_user_agent`]), "default"/None -> None (the engine's own UA), anything else -> literal.
pub(crate) fn resolve_user_agent_preset(preset: Option<&str>, platform: &str) -> Option<String> {
    match preset {
        None | Some("default") => None,
        Some("webkit") => Some(webkit_user_agent(platform)),
        Some("chrome") => Some(chrome_user_agent(platform)),
        Some(other) => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLATFORM: &str = "X11; Linux aarch64";

    #[test]
    fn user_agents_carry_the_hosts_platform() {
        assert_eq!(
            webkit_user_agent(PLATFORM),
            "Mozilla/5.0 (X11; Linux aarch64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.1 Safari/605.1.15"
        );
        assert!(chrome_user_agent(PLATFORM).starts_with("Mozilla/5.0 (X11; Linux aarch64) "));
    }
}
