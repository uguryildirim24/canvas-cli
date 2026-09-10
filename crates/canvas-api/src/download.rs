//! File download transport (§11).

use futures_util::StreamExt;
use reqwest::Url;
use reqwest::header::{ACCEPT_ENCODING, HeaderMap, HeaderValue, LOCATION};
use reqwest::{Method, StatusCode};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::governor::{Lane, retry_delays};
use crate::request::{is_rate_limited, observe_headers, parse_retry_after, validate_transfer_url};
use crate::{Client, Error};

/// Download `url` into `sink` per §11 transfer rules.
///
/// Returns bytes written. `on_progress` receives cumulative bytes.
pub async fn download(
    client: &Client,
    url: Url,
    mut sink: impl AsyncWrite + Unpin + Send,
    expected_size: Option<u64>,
    mut on_progress: impl FnMut(u64) + Send,
) -> Result<u64, Error> {
    validate_transfer_url(&url)?;

    let mut attempt = 0_u32;
    let mut retry_after = None;
    loop {
        match download_once(
            client,
            url.clone(),
            &mut sink,
            expected_size,
            &mut on_progress,
            &mut retry_after,
        )
        .await
        {
            Err(
                Error::RateLimited
                | Error::Forbidden {
                    rate_limited: true, ..
                },
            ) if attempt < 4 => {
                let _delay = retry_delays(attempt, retry_after, client.governor().jitter()).await;
                attempt += 1;
            }
            other => return other,
        }
    }
}

async fn download_once(
    client: &Client,
    start: Url,
    sink: &mut (impl AsyncWrite + Unpin + Send),
    expected_size: Option<u64>,
    on_progress: &mut (impl FnMut(u64) + Send),
    retry_after: &mut Option<std::time::Duration>,
) -> Result<u64, Error> {
    let mut url = start;
    for hop in 0..=5 {
        validate_transfer_url(&url)?;
        let permit = client
            .governor()
            .admit(Lane::Storage, "transfer:download")
            .await;
        let mut builder = client.transfer_http().request(Method::GET, url.clone());
        builder = builder.header(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        if let Some(auth) = client.auth_header_for(&url) {
            builder = builder.header(reqwest::header::AUTHORIZATION, auth);
        }

        let response = builder.send().await.map_err(|e| map_err(&e))?;
        let status = response.status();
        let headers = response.headers().clone();
        observe_headers(client, permit.issue(), &headers);

        if is_redirect(status) {
            if hop == 5 {
                return Err(Error::UnexpectedRedirect);
            }
            let location = location_url(&url, &headers)?;
            validate_transfer_url(&location)?;
            url = location;
            continue;
        }

        if status.is_redirection() {
            return Err(Error::UnexpectedRedirect);
        }
        if !status.is_success() {
            // Only 403 needs a body to distinguish a throttle from a denial.
            // A broken error body must not turn a final 401/404 into Network.
            let throttled = if status == StatusCode::FORBIDDEN {
                response
                    .bytes()
                    .await
                    .is_ok_and(|bytes| is_rate_limited(status, &bytes))
            } else {
                status == StatusCode::TOO_MANY_REQUESTS
            };
            if throttled {
                *retry_after = parse_retry_after(&headers);
                return Err(Error::RateLimited);
            }
            return Err(classify_download(client, &url, status, &headers));
        }

        let content_length = headers
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        let mut stream = response.bytes_stream();
        let mut written = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                if !e.is_timeout() && content_length.is_some_and(|n| n != written) {
                    Error::SizeMismatch
                } else {
                    map_err(&e)
                }
            })?;
            sink.write_all(&chunk).await.map_err(|_| Error::Network)?;
            written += chunk.len() as u64;
            on_progress(written);
        }
        sink.flush().await.map_err(|_| Error::Network)?;

        if let Some(cl) = content_length
            && cl != written
        {
            return Err(Error::SizeMismatch);
        }
        if let Some(expected) = expected_size
            && expected != written
        {
            return Err(Error::SizeMismatch);
        }
        return Ok(written);
    }
    Err(Error::UnexpectedRedirect)
}

fn classify_download(
    client: &Client,
    url: &Url,
    status: StatusCode,
    _headers: &HeaderMap,
) -> Error {
    let code = status.as_u16();
    if code == 429 {
        return Error::RateLimited;
    }
    if client.same_origin(url) {
        Error::Denied { status: code }
    } else if code == 403 {
        Error::StorageExpired
    } else {
        Error::Denied { status: code }
    }
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

fn location_url(current: &Url, headers: &HeaderMap) -> Result<Url, Error> {
    let raw = headers
        .get(LOCATION)
        .ok_or(Error::UnexpectedRedirect)?
        .to_str()
        .map_err(|_| Error::UnexpectedRedirect)?;
    current
        .join(raw)
        .or_else(|_| Url::parse(raw))
        .map_err(|_| Error::UnexpectedRedirect)
}

fn map_err(err: &reqwest::Error) -> Error {
    if err.is_timeout() {
        Error::Timeout
    } else {
        Error::Network
    }
}
