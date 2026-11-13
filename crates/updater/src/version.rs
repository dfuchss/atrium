//! Release versions.

use std::fmt;

/// A release version `major.minor.patch`. Pre-release and build suffixes do not parse, so such
/// releases are never offered as updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// Parses a version (`0.2.0`) or a release tag (`v0.2.0`).
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let version = Version {
            major: number(parts.next()?)?,
            minor: number(parts.next()?)?,
            patch: number(parts.next()?)?,
        };
        parts.next().is_none().then_some(version)
    }

    /// The release tag of this version, `v<major>.<minor>.<patch>`.
    pub fn tag(self) -> String {
        format!("v{self}")
    }
}

/// Digits only: no sign, no whitespace, no suffix.
fn number(part: &str) -> Option<u64> {
    if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    #[test]
    fn parses_versions_and_tags() {
        assert_eq!(Version::parse("0.1.0"), Some(version(0, 1, 0)));
        assert_eq!(Version::parse("v1.20.3"), Some(version(1, 20, 3)));
        assert_eq!(Version::parse(" v2.0.10 "), Some(version(2, 0, 10)));
        assert_eq!(version(1, 2, 3).tag(), "v1.2.3");
    }

    #[test]
    fn rejects_prereleases_and_malformed_versions() {
        for text in [
            "",
            "v",
            "1.2",
            "1.2.3.4",
            "1.2.3-rc.1",
            "1.2.3+build",
            "1..3",
            "a.b.c",
            "+1.2.3",
            "1.2.-3",
            "vv1.2.3",
            "../1.2.3",
        ] {
            assert_eq!(Version::parse(text), None, "{text}");
        }
    }

    #[test]
    fn compares_numerically() {
        assert!(version(0, 10, 0) > version(0, 9, 9));
        assert!(version(1, 0, 0) > version(0, 99, 99));
        assert!(version(0, 1, 1) > version(0, 1, 0));
        assert_eq!(version(0, 1, 0), Version::parse("v0.1.0").unwrap());
    }
}
