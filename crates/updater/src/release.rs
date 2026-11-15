//! Release metadata: the GitHub release JSON, the asset for this Mac and `SHA256SUMS`.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::version::Version;

/// GitHub API endpoint of the latest release (drafts and pre-releases are never "latest").
pub(crate) const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/dfuchss/atrium/releases/latest";
/// Shown when a release names no (acceptable) page of its own.
pub(crate) const RELEASES_PAGE: &str = "https://github.com/dfuchss/atrium/releases/latest";
/// Release pages and downloads are only taken from the project's own release URLs.
const RELEASE_PAGE_PREFIX: &str = "https://github.com/dfuchss/atrium/releases/";
const DOWNLOAD_PREFIX: &str = "https://github.com/dfuchss/atrium/releases/download/";
/// The checksum file every release publishes (`sha256sum` output over the release files).
pub(crate) const SHA256SUMS: &str = "SHA256SUMS";

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    html_url: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<AssetJson>,
}

#[derive(Deserialize)]
struct AssetJson {
    name: String,
    browser_download_url: String,
}

/// A release that can be offered as an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Release {
    pub(crate) version: Version,
    pub(crate) page_url: String,
    /// Asset name to download URL; only URLs below the project's release downloads.
    pub(crate) assets: BTreeMap<String, String>,
}

impl Release {
    /// Decodes `releases/latest`. `Ok(None)` for drafts, pre-releases and tags that are no
    /// plain `vX.Y.Z` version; `Err` when the body is no release JSON at all.
    pub(crate) fn parse(body: &str) -> Result<Option<Release>, String> {
        let json: ReleaseJson =
            serde_json::from_str(body).map_err(|error| format!("bad release JSON: {error}"))?;
        if json.draft || json.prerelease {
            return Ok(None);
        }
        let Some(version) = Version::parse(&json.tag_name) else {
            return Ok(None);
        };
        let page_url = json
            .html_url
            .filter(|url| url.starts_with(RELEASE_PAGE_PREFIX))
            .unwrap_or_else(|| RELEASES_PAGE.to_string());
        let assets = json
            .assets
            .into_iter()
            .filter(|asset| asset.browser_download_url.starts_with(DOWNLOAD_PREFIX))
            .map(|asset| (asset.name, asset.browser_download_url))
            .collect();
        Ok(Some(Release {
            version,
            page_url,
            assets,
        }))
    }
}

/// The self-update asset for the given system and architecture (`std::env::consts::OS` and
/// `ARCH`): only Macs update themselves. The names are fixed by `.github/workflows/release.yml`.
pub fn asset_name(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("atrium-macos-arm64.zip"),
        ("macos", "x86_64") => Some("atrium-macos-x86_64.zip"),
        _ => None,
    }
}

/// Parses `sha256sum` output into file name (without directory) to lower case hex digest;
/// other lines are ignored.
pub(crate) fn parse_sha256sums(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (digest, path) = line.trim().split_once(char::is_whitespace)?;
            let is_digest =
                digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit());
            // `sha256sum -b` marks binary mode with a `*` before the name.
            let path = path.trim_start().trim_start_matches('*');
            let name = path.rsplit('/').next().unwrap_or(path);
            (is_digest && !name.is_empty()).then(|| (name.to_string(), digest.to_ascii_lowercase()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "0123456789abcdef0123456789ABCDEF0123456789abcdef0123456789abcdef";

    fn release_json(tag: &str, extra: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://github.com/dfuchss/atrium/releases/tag/{tag}",{extra}
            "assets":[
              {{"name":"atrium-macos-arm64.zip","browser_download_url":"https://github.com/dfuchss/atrium/releases/download/{tag}/atrium-macos-arm64.zip","size":1}},
              {{"name":"SHA256SUMS","browser_download_url":"https://github.com/dfuchss/atrium/releases/download/{tag}/SHA256SUMS"}},
              {{"name":"evil.zip","browser_download_url":"https://example.com/evil.zip"}}
            ]}}"#
        )
    }

    #[test]
    fn decodes_a_release_and_keeps_only_project_downloads() {
        let release = Release::parse(&release_json("v0.2.0", ""))
            .unwrap()
            .unwrap();
        assert_eq!(release.version, Version::parse("0.2.0").unwrap());
        assert_eq!(
            release.page_url,
            "https://github.com/dfuchss/atrium/releases/tag/v0.2.0"
        );
        assert_eq!(
            release.assets.keys().collect::<Vec<_>>(),
            ["SHA256SUMS", "atrium-macos-arm64.zip"]
        );
    }

    #[test]
    fn ignores_drafts_prereleases_and_other_tags() {
        for (tag, extra) in [
            ("v0.2.0", r#""draft":true,"#),
            ("v0.2.0", r#""prerelease":true,"#),
            ("v0.2.0-rc.1", ""),
            ("nightly", ""),
        ] {
            assert_eq!(Release::parse(&release_json(tag, extra)).unwrap(), None);
        }
        assert!(Release::parse("<html>").is_err());
    }

    #[test]
    fn foreign_release_page_falls_back() {
        let body = r#"{"tag_name":"v1.0.0","html_url":"https://example.com/x","assets":[]}"#;
        assert_eq!(
            Release::parse(body).unwrap().unwrap().page_url,
            RELEASES_PAGE
        );
    }

    #[test]
    fn picks_the_asset_of_the_mac_architecture() {
        assert_eq!(
            asset_name("macos", "aarch64"),
            Some("atrium-macos-arm64.zip")
        );
        assert_eq!(
            asset_name("macos", "x86_64"),
            Some("atrium-macos-x86_64.zip")
        );
        assert_eq!(asset_name("macos", "riscv64"), None);
        assert_eq!(asset_name("linux", "x86_64"), None);
        assert_eq!(asset_name("windows", "x86_64"), None);
    }

    #[test]
    fn parses_sha256sum_output() {
        let text = format!(
            "{DIGEST}  atrium-macos-arm64.zip\n{DIGEST} *dist/atrium-macos-x86_64.zip\n\nnot a line\nabc  short.zip\n"
        );
        let sums = parse_sha256sums(&text);
        assert_eq!(sums.len(), 2);
        assert_eq!(sums["atrium-macos-arm64.zip"], DIGEST.to_ascii_lowercase());
        assert!(sums.contains_key("atrium-macos-x86_64.zip"));
    }
}
