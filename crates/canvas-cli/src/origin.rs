//! Canonical Canvas origin (§8).

use reqwest::Url;

use crate::exit::CliError;

/// Normalize a host or URL to a canonical origin string.
///
/// Form: `https://` + lowercase IDNA-ASCII host + `:port` only when the port is
/// not 443. HTTP loopback is available only in debug builds with the test gate.
pub fn canonicalize_origin(raw: &str) -> Result<String, CliError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CliError::usage("host must not be empty"));
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };

    let url = Url::parse(&candidate).map_err(|e| CliError::usage(format!("invalid host: {e}")))?;
    let scheme = url.scheme();
    if scheme != "https" && scheme != "http" {
        return Err(CliError::usage(format!(
            "unsupported origin scheme `{scheme}`"
        )));
    }
    if scheme == "http"
        && !(cfg!(debug_assertions)
            && std::env::var("CANVAS_TEST_ALLOW_HTTP").ok().as_deref() == Some("1")
            && url
                .host_str()
                .is_some_and(|host| matches!(host, "127.0.0.1" | "[::1]")))
    {
        return Err(CliError::usage("Canvas origin must use https"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CliError::usage("origin must not include credentials"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(CliError::usage(
            "origin must not include a query or fragment",
        ));
    }

    // `host_str` is IDNA-ASCII for domains and bracketed for IPv6.
    let host = url
        .host_str()
        .ok_or_else(|| CliError::usage("origin must include a host"))?
        .to_ascii_lowercase();

    let default_port = match scheme {
        "https" => Some(443),
        "http" => Some(80),
        _ => None,
    };
    let authority = match (url.port(), default_port) {
        (Some(p), Some(d)) if p == d => host,
        (Some(p), _) => format!("{host}:{p}"),
        (None, _) => host,
    };

    Ok(format!("{scheme}://{authority}"))
}

/// Parse a stored origin into a [`Url`] suitable for [`canvas_api::Client`].
pub fn origin_url(origin: &str) -> Result<Url, CliError> {
    let mut url = Url::parse(origin)
        .map_err(|e| CliError::usage(format!("invalid origin `{origin}`: {e}")))?;
    if url.path().is_empty() {
        url.set_path("/");
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_default_and_port_rules() {
        assert_eq!(
            canonicalize_origin("Canvas.Example.EDU").unwrap(),
            "https://canvas.example.edu"
        );
        assert_eq!(
            canonicalize_origin("https://example.test:443").unwrap(),
            "https://example.test"
        );
        assert_eq!(
            canonicalize_origin("https://example.test:8443").unwrap(),
            "https://example.test:8443"
        );
    }

    #[test]
    fn ipv6_origin() {
        assert_eq!(
            canonicalize_origin("https://[::1]:8443").unwrap(),
            "https://[::1]:8443"
        );
    }

    #[test]
    fn http_refused_without_test_gate() {
        assert!(canonicalize_origin("http://example.test").is_err());
    }
}
