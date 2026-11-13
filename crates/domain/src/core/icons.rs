//! Sidebar icons: site favicons found in the page and Simple Icons glyphs.

use serde_json::Value;
use uuid::Uuid;

use super::{Core, Effect, ICON_SCRIPT, SimpleIconResult};

/// Favicon lookups per account and app run.
const MAX_FAVICON_ATTEMPTS: u8 = 3;

const IMAGE_DATA_URL_PREFIX: &str = "data:image/";

/// URL of the white glyph of a Simple Icons slug.
pub fn simple_icon_url(slug: &str) -> String {
    format!("https://cdn.simpleicons.org/{slug}/white")
}

/// Normalises a Simple Icons slug: trimmed, lowercase, `[a-z0-9]` only. `Ok(None)` = empty.
pub fn normalize_slug(raw: &str) -> Result<Option<String>, String> {
    let slug = raw.trim().to_lowercase();
    if slug.is_empty() {
        Ok(None)
    } else if slug
        .chars()
        .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
    {
        Ok(Some(slug))
    } else {
        Err(format!(
            "invalid Simple Icons slug '{}' (only a-z and 0-9)",
            raw.trim()
        ))
    }
}

/// The icon URL in the result of [`ICON_SCRIPT`]: a JSON string, possibly encoded twice.
fn decode_icon_url(raw: &str) -> Option<String> {
    let mut value = serde_json::from_str::<Value>(raw).ok();
    if let Some(Value::String(inner)) = &value
        && inner.starts_with('"')
    {
        value = serde_json::from_str(inner).ok();
    }
    match value {
        Some(Value::String(url)) if !url.is_empty() => Some(url),
        _ => None,
    }
}

impl Core {
    /// Looks for a favicon after a page load until one was fetched.
    pub(super) fn page_loaded(&mut self, account_id: Uuid) -> Vec<Effect> {
        let Some(account) = self.account(account_id) else {
            return vec![];
        };
        if account.favicon.is_some() || !self.is_loaded(account_id) {
            return vec![];
        }
        // Single-page apps often set their icon only after the first load: allow a few attempts,
        // each on a later page load (a successful fetch sets `favicon` and stops this).
        let attempts = self.favicon_attempts.entry(account_id).or_insert(0);
        if *attempts >= MAX_FAVICON_ATTEMPTS {
            return vec![];
        }
        *attempts += 1;
        vec![Effect::FindIcon {
            account: account_id,
            script: ICON_SCRIPT.into(),
        }]
    }

    pub(super) fn icon_found(&mut self, account_id: Uuid, raw: &str) -> Vec<Effect> {
        if !self.has_account(account_id) {
            return vec![Self::unknown(account_id)];
        }
        let Some(url) = decode_icon_url(raw) else {
            return vec![Effect::Log(format!(
                "icon of account {account_id}: no icon URL ({raw:.100})"
            ))];
        };
        if url.starts_with(IMAGE_DATA_URL_PREFIX) {
            return self.icon_fetched(account_id, Some(url));
        }
        match url::Url::parse(&url) {
            Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => {
                vec![Effect::FetchIcon {
                    account: account_id,
                    url,
                }]
            }
            _ => vec![Effect::Log(format!(
                "icon of account {account_id}: unsupported URL {url:.100}"
            ))],
        }
    }

    pub(super) fn icon_fetched(
        &mut self,
        account_id: Uuid,
        data_url: Option<String>,
    ) -> Vec<Effect> {
        let Some(account) = self.account_mut(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        match data_url {
            Some(data_url) if data_url.starts_with(IMAGE_DATA_URL_PREFIX) => {
                if account.favicon.as_deref() == Some(data_url.as_str()) {
                    return vec![];
                }
                account.favicon = Some(data_url);
                vec![self.persist(), self.emit()]
            }
            _ => vec![Effect::Log(format!(
                "icon of account {account_id}: download failed"
            ))],
        }
    }

    /// Downloads for the accounts whose chosen glyph is not cached yet.
    pub(super) fn fetch_missing_simple_icons(&self) -> Vec<Effect> {
        self.persisted
            .accounts
            .iter()
            .filter(|account| account.simple_icon_data.is_none())
            .filter_map(|account| {
                let slug = account.simple_icon.clone()?;
                Some(Effect::FetchSimpleIcon {
                    account: account.id,
                    slug,
                })
            })
            .collect()
    }

    pub(super) fn simple_icon_fetched(
        &mut self,
        account_id: Uuid,
        slug: &str,
        result: SimpleIconResult,
    ) -> Vec<Effect> {
        let Some(account) = self.account_mut(account_id) else {
            return vec![Self::unknown(account_id)];
        };
        if account.simple_icon.as_deref() != Some(slug) {
            return vec![]; // Stale: the slug changed meanwhile.
        }
        match result {
            SimpleIconResult::Found(data_url) if data_url.starts_with(IMAGE_DATA_URL_PREFIX) => {
                account.simple_icon_data = Some(data_url);
                vec![self.persist(), self.emit()]
            }
            SimpleIconResult::Found(_) => {
                vec![Effect::Log(format!("Simple Icons '{slug}': not an image"))]
            }
            SimpleIconResult::NotFound => {
                vec![Effect::Log(format!("Simple Icons has no icon '{slug}'"))]
            }
            SimpleIconResult::Failed(error) => {
                vec![Effect::Log(format!(
                    "Simple Icons '{slug}' failed: {error}"
                ))]
            }
        }
    }
}
