//! Where releases and their files come from.

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use crate::release::LATEST_RELEASE_API;

/// Largest accepted release JSON.
const MAX_RELEASE_JSON_BYTES: u64 = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
/// A stalled download fails after this long without data.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// An open download; `length` is `None` when the server does not tell.
pub struct Download {
    pub reader: Box<dyn Read + Send>,
    pub length: Option<u64>,
}

/// Blocking access to the releases; errors are messages for the log and the UI.
pub trait ReleaseSource: Send + Sync {
    /// The body of the latest-release API response.
    fn latest_release(&self) -> Result<String, String>;
    /// Opens `url`; reading more than `max_bytes` fails.
    fn open(&self, url: &str, max_bytes: u64) -> Result<Download, String>;
}

/// The GitHub releases API over `ureq` (rustls).
pub struct GitHubSource {
    agent: ureq::Agent,
}

impl GitHubSource {
    pub fn new(app_version: &str) -> GitHubSource {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(RESPONSE_TIMEOUT))
            .timeout_per_read(Some(READ_TIMEOUT))
            .user_agent(format!("Atrium/{app_version} (updater)"))
            .build()
            .into();
        GitHubSource { agent }
    }
}

impl ReleaseSource for GitHubSource {
    fn latest_release(&self) -> Result<String, String> {
        let mut response = self
            .agent
            .get(LATEST_RELEASE_API)
            .header("Accept", "application/vnd.github+json")
            .call()
            .map_err(|error| format!("GitHub: {error}"))?;
        response
            .body_mut()
            .with_config()
            .limit(MAX_RELEASE_JSON_BYTES)
            .read_to_string()
            .map_err(|error| format!("GitHub: {error}"))
    }

    fn open(&self, url: &str, max_bytes: u64) -> Result<Download, String> {
        let response = self
            .agent
            .get(url)
            .call()
            .map_err(|error| format!("download: {error}"))?;
        let body = response.into_body();
        let length = body.content_length();
        Ok(Download {
            reader: Box::new(body.into_with_config().limit(max_bytes).reader()),
            length,
        })
    }
}

/// A local folder standing in for the latest release, for testing updates by hand
/// (`ATRIUM_DEV_RELEASE_DIR` in debug builds): `release.json` is the latest-release response,
/// and every download URL is served from the file named like its last path segment
/// (`SHA256SUMS`, the asset).
pub struct DirectorySource {
    dir: PathBuf,
}

impl DirectorySource {
    pub fn new(dir: PathBuf) -> DirectorySource {
        DirectorySource { dir }
    }
}

impl ReleaseSource for DirectorySource {
    fn latest_release(&self) -> Result<String, String> {
        let path = self.dir.join("release.json");
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))
    }

    fn open(&self, url: &str, max_bytes: u64) -> Result<Download, String> {
        let name = url.rsplit('/').next().unwrap_or_default();
        if matches!(name, "" | "." | "..") {
            return Err(format!("no file name in {url}"));
        }
        let path = self.dir.join(name);
        let failed = |error: std::io::Error| format!("{}: {error}", path.display());
        let file = File::open(&path).map_err(failed)?;
        let length = file.metadata().map_err(failed)?.len();
        Ok(Download {
            reader: Box::new(file.take(max_bytes)),
            length: Some(length),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_serves_the_release_and_its_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("release.json"), r#"{"tag_name":"v9.9.9"}"#).unwrap();
        std::fs::write(dir.path().join("SHA256SUMS"), "0123456789").unwrap();
        let source = DirectorySource::new(dir.path().to_path_buf());
        assert!(source.latest_release().unwrap().contains("v9.9.9"));

        let url = "https://github.com/dfuchss/atrium/releases/download/v9.9.9/SHA256SUMS";
        let mut download = source.open(url, 4).unwrap();
        assert_eq!(download.length, Some(10));
        let mut text = String::new();
        download.reader.read_to_string(&mut text).unwrap();
        assert_eq!(text, "0123");

        assert!(source.open("https://example.com/missing", 4).is_err());
        assert!(source.open("https://example.com/download/..", 4).is_err());
        assert!(source.open("https://example.com/download/", 4).is_err());
    }
}
