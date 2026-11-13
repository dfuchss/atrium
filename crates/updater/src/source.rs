//! Where releases and their files come from.

use std::io::Read;
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
