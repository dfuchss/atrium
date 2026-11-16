//! Favicon download: small blocking `ureq` client (rustls), run on a background thread.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs as _};
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

fn io_error(message: impl Into<String>) -> ureq::Error {
    ureq::Error::Io(std::io::Error::other(message.into()))
}

/// True for addresses that belong to the local machine or network, never to a public icon host.
pub(crate) fn is_local_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_local_v4(v4),
        IpAddr::V6(v6) => is_local_v6(v6),
    }
}

fn is_local_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        || (a == 100 && (64..128).contains(&b))
}

fn is_local_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_local_v4(v4);
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (ip.segments()[0] & 0xfe00) == 0xfc00
        || (ip.segments()[0] & 0xffc0) == 0xfe80
}

/// Rejects URLs whose host is, or resolves to, a local or private address.
fn ensure_public_host(url: &url::Url) -> Result<(), ureq::Error> {
    let host = url.host_str().ok_or_else(|| io_error("no host"))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses = (host.trim_matches(['[', ']']), port)
        .to_socket_addrs()
        .map_err(ureq::Error::Io)?;
    for address in addresses {
        if is_local_address(address.ip()) {
            return Err(io_error(format!("{host} is a local address")));
        }
    }
    Ok(())
}

fn download(url: &str) -> Result<String, ureq::Error> {
    let target = url::Url::parse(url).map_err(|error| io_error(error.to_string()))?;
    if !matches!(target.scheme(), "http" | "https") {
        return Err(io_error("scheme not allowed"));
    }
    ensure_public_host(&target)?;
    fetch(url)
}

fn fetch(url: &str) -> Result<String, ureq::Error> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(domain::catalog::webkit_user_agent(
            &crate::native::user_agent_platform(),
        ))
        // Only the requested URL passed the host allow-list (`domain::core::icons`); a redirect
        // could lead anywhere, so it is returned as is and refused below.
        .max_redirects(0)
        .build()
        .into();
    let mut response = agent.get(url).call()?;
    if response.status().is_redirection() {
        return Err(ureq::Error::Io(std::io::Error::other(
            "redirects are not followed",
        )));
    }
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
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn redirects_are_not_followed() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 302 Found\r\nLocation: http://other.example/i.png\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        let error = fetch(&format!("http://{address}/favicon.ico"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("redirect"), "{error}");
        server.join().unwrap();
    }

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

    #[test]
    fn local_addresses_are_rejected() {
        for local in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:127.0.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "192.0.2.1",
            "0.1.2.3",
            "100.127.255.255",
            "ff02::1",
            "fc00::1",
            "febf::1",
            "::",
        ] {
            assert!(is_local_address(local.parse().unwrap()), "{local}");
        }
        for public in [
            "8.8.8.8",
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(!is_local_address(public.parse().unwrap()), "{public}");
        }
    }

    #[test]
    fn only_http_urls_are_fetched() {
        assert!(download_icon("ftp://8.8.8.8/icon.ico").is_err());
        assert!(download_icon("file:///etc/hosts").is_err());
    }

    #[test]
    fn loopback_urls_are_not_fetched() {
        assert!(download_icon("http://127.0.0.1:1/favicon.ico").is_err());
        assert!(download_icon("http://[::1]/favicon.ico").is_err());
    }
}
