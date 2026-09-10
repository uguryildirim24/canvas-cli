//! File download transport (§11).

use futures_util::StreamExt;
use reqwest::Url;
use reqwest::header::{ACCEPT_ENCODING, HeaderMap, HeaderValue, LOCATION};
use reqwest::{Method, StatusCode};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::governor::{Lane, retry_delays};
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
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::Network);
    }

    let mut attempt = 0_u32;
    loop {
        match download_once(client, url.clone(), &mut sink, expected_size, &mut on_progress).await
        {
            Err(Error::RateLimited) if attempt < 4 => {
                let _delay = retry_delays(attempt, None, client.governor().jitter()).await;
                attempt += 1;
            }
            Err(Error::Forbidden {
                rate_limited: true,
                ..
            }) if attempt < 4 => {
                let _delay = retry_delays(attempt, None, client.governor().jitter()).await;
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
) -> Result<u64, Error> {
    let permit = client
        .governor()
        .admit(Lane::Storage, "transfer:download")
        .await;
    let issue = permit.issue();

    let mut url = start;
    for hop in 0..=5 {
        if hop > 5 {
            return Err(Error::UnexpectedRedirect);
        }
        if !matches!(url.scheme(), "http" | "https") {
            return Err(Error::Network);
        }

        let mut builder = client.transfer_http().request(Method::GET, url.clone());
        builder = builder.header(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        if let Some(auth) = client.auth_header_for(&url) {
            builder = builder.header(reqwest::header::AUTHORIZATION, auth);
        }

        let response = builder.send().await.map_err(map_err)?;
        let status = response.status();
        let headers = response.headers().clone();
        observe(client, issue, &headers);

        if is_redirect(status) {
            if hop == 5 {
                return Err(Error::UnexpectedRedirect);
            }
            let location = location_url(&url, &headers)?;
            if !matches!(location.scheme(), "http" | "https") {
                return Err(Error::Network);
            }
            url = location;
            continue;
        }

        // Classification order: throttle already handled via retries above;
        // Canvas-origin 401/403/404 → Denied; storage 403 → StorageExpired.
        if !status.is_success() {
            return Err(classify_download(client, &url, status, &headers));
        }

        let content_length = headers
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        let mut stream = response.bytes_stream();
        let mut written = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_err)?;
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
        match code {
            401 | 403 | 404 => Error::Denied { status: code },
            _ => Error::Denied { status: code },
        }
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

fn observe(client: &Client, issue: u64, headers: &HeaderMap) {
    let remaining = headers
        .get("x-rate-limit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());
    let cost = headers
        .get("x-request-cost")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());
    if let Some(remaining) = remaining {
        client.governor().observe(issue, remaining, cost);
    } else if let Some(cost) = cost {
        client.governor().observe_cost_only(issue, cost);
    }
}

fn map_err(err: reqwest::Error) -> Error {
    if err.is_timeout() {
        Error::Timeout
    } else {
        Error::Network
    }
}
