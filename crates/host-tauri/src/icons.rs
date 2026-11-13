//! Favicon download: small blocking `ureq` client (rustls), run on a background thread.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

/// Larger responses are rejected.
pub const MAX_ICON_BYTES: u64 = 512 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);

/// Downloads `url` and returns it as `data:<mime>;base64,...`; only `image/*` responses.
pub fn download_icon(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(domain::catalog::UA_SAFARI)
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).map(String::from);
    let mime = image_mime(content_type.as_deref())?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_ICON_BYTES)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("empty response".into());
    }
    Ok(data_url(&mime, &bytes))
}

/// The MIME type of an `image/*` content type (parameters dropped, lowercased).
pub fn image_mime(content_type: Option<&str>) -> Result<String, String> {
    let mime = content_type.unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if mime.starts_with("image/") && mime.len() > "image/".len() {
        Ok(mime)
    } else {
        Err(format!("not an image: {mime:?}"))
    }
}

pub fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_filter_and_encoding() {
        assert_eq!(image_mime(Some("Image/PNG; charset=binary")).unwrap(), "image/png");
        assert_eq!(image_mime(Some("image/svg+xml")).unwrap(), "image/svg+xml");
        assert!(image_mime(Some("text/html")).is_err());
        assert!(image_mime(Some("image/")).is_err());
        assert!(image_mime(None).is_err());
        assert_eq!(data_url("image/png", &[0, 1, 2]), "data:image/png;base64,AAEC");
    }
}
