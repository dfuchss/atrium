//! Favicon download: small blocking `ureq` client (rustls), run on a background thread.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domain::core::{SimpleIconResult, simple_icon_url};

/// Larger responses are rejected.
pub(crate) const MAX_ICON_BYTES: u64 = 512 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);

/// Downloads `url` and returns it as `data:<mime>;base64,...`; only `image/*` responses.
pub(crate) fn download_icon(url: &str) -> Result<String, String> {
    download(url).map_err(|error| error.to_string())
}

/// Downloads the Simple Icons glyph `slug` (white SVG).
pub(crate) fn download_simple_icon(slug: &str) -> SimpleIconResult {
    match download(&simple_icon_url(slug)) {
        Ok(data_url) => SimpleIconResult::Found(data_url),
        Err(ureq::Error::StatusCode(404)) => SimpleIconResult::NotFound,
        Err(error) => SimpleIconResult::Failed(error.to_string()),
    }
}

fn download(url: &str) -> Result<String, ureq::Error> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(domain::catalog::UA_SAFARI)
        .build()
        .into();
    let mut response = agent.get(url).call()?;
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(String::from);
    let mime = image_mime(content_type.as_deref())
        .map_err(|error| ureq::Error::Io(std::io::Error::other(error)))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_ICON_BYTES)
        .read_to_vec()?;
    if bytes.is_empty() {
        return Err(ureq::Error::Io(std::io::Error::other("empty response")));
    }
    Ok(data_url(&mime, &bytes))
}

/// The MIME type of an `image/*` content type (parameters dropped, lowercased).
pub(crate) fn image_mime(content_type: Option<&str>) -> Result<String, String> {
    let mime = content_type
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if mime.starts_with("image/") && mime.len() > "image/".len() {
        Ok(mime)
    } else {
        Err(format!("not an image: {mime:?}"))
    }
}

pub(crate) fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_filter_and_encoding() {
        assert_eq!(
            image_mime(Some("Image/PNG; charset=binary")).unwrap(),
            "image/png"
        );
        assert_eq!(image_mime(Some("image/svg+xml")).unwrap(), "image/svg+xml");
        assert!(image_mime(Some("text/html")).is_err());
        assert!(image_mime(Some("image/")).is_err());
        assert!(image_mime(None).is_err());
        assert_eq!(
            data_url("image/png", &[0, 1, 2]),
            "data:image/png;base64,AAEC"
        );
    }
}
