//! API and transfer request builders and executors (SPEC §11 phases).

use std::time::Duration;

use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, LOCATION};
use reqwest::{Method, StatusCode};
use serde::Serialize;

use crate::governor::{Lane, retry_delays};
use crate::{Client, Error, TransferResponse};

/// Builder for a Canvas API request (`/api/v1` and related).
#[derive(Clone)]
pub struct ApiRequest {
    pub(crate) method: Method,
    pub(crate) url: Url,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Option<Vec<u8>>,
    pub(crate) route_key: String,
}

impl ApiRequest {
    /// Start an API request to `url`.
    #[must_use]
    pub fn new(method: Method, url: Url) -> Self {
        let route_key = route_key_for(&method, &url);
        Self {
            method,
            url,
            headers: HeaderMap::new(),
            body: None,
            route_key,
        }
    }

    /// Override the governor route key used for cost pre-charge.
    #[must_use]
    pub fn route_key(mut self, key: impl Into<String>) -> Self {
        self.route_key = key.into();
        self
    }

    /// Attach a JSON body.
    pub fn json<B: Serialize + ?Sized>(mut self, body: &B) -> Result<Self, Error> {
        let bytes = serde_json::to_vec(body).map_err(|_| Error::Decode)?;
        self.headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/json"),
        );
        self.body = Some(bytes);
        Ok(self)
    }

    /// Attach raw body bytes.
    #[must_use]
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }
}

/// Kind of transfer-phase request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    /// Multipart upload `POST` to a storage `upload_url` (never auto-followed).
    Upload,
    /// Download `GET` with hop following.
    Download,
}

/// Builder for a transfer-phase request.
#[derive(Clone)]
pub struct TransferRequest {
    pub(crate) kind: TransferKind,
    pub(crate) method: Method,
    pub(crate) url: Url,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Option<Vec<u8>>,
    pub(crate) route_key: String,
}

impl TransferRequest {
    /// Upload POST (completion handoff is caller-owned; redirects are not followed).
    #[must_use]
    pub fn upload(url: Url) -> Self {
        Self {
            kind: TransferKind::Upload,
            method: Method::POST,
            route_key: "transfer:upload".into(),
            url,
            headers: HeaderMap::new(),
            body: None,
        }
    }

    /// Download GET (follows https hops per transfer rules).
    #[must_use]
    pub fn download(url: Url) -> Self {
        Self {
            kind: TransferKind::Download,
            method: Method::GET,
            route_key: "transfer:download".into(),
            url,
            headers: HeaderMap::new(),
            body: None,
        }
    }

    /// Override governor route key.
    #[must_use]
    pub fn route_key(mut self, key: impl Into<String>) -> Self {
        self.route_key = key.into();
        self
    }

    /// Attach raw body bytes (upload multipart body).
    #[must_use]
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    /// Insert a header.
    #[must_use]
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }
}

pub(crate) async fn execute_api(
    client: &Client,
    request: ApiRequest,
) -> Result<(StatusCode, HeaderMap, Vec<u8>), Error> {
    let mut attempt = 0_u32;
    loop {
        let result = execute_api_once(client, &request).await;
        match result {
            Ok((status, headers, body)) if is_rate_limited(status, &body) && attempt < 4 => {
                let retry_after = parse_retry_after(&headers);
                let _delay = retry_delays(attempt, retry_after, client.governor().jitter()).await;
                attempt += 1;
            }
            other => return other,
        }
    }
}

async fn execute_api_once(
    client: &Client,
    request: &ApiRequest,
) -> Result<(StatusCode, HeaderMap, Vec<u8>), Error> {
    let permit = client.governor().admit(Lane::Api, &request.route_key).await;
    let issue = permit.issue();

    let mut method = request.method.clone();
    let mut url = request.url.clone();
    let mut body = request.body.clone();
    let original_was_get = request.method == Method::GET;

    for hop in 0..=5 {
        if hop > 5 {
            return Err(Error::UnexpectedRedirect);
        }
        if !client.same_origin(&url) {
            return Err(Error::CrossOrigin);
        }

        let mut builder = client.http().request(method.clone(), url.clone());
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(auth) = client.auth_header_for(&url) {
            builder = builder.header(reqwest::header::AUTHORIZATION, auth);
        }
        if let Some(ref bytes) = body {
            builder = builder.body(bytes.clone());
        }

        let response = builder.send().await.map_err(|e| map_reqwest_error(&e))?;
        let status = response.status();
        let headers = response.headers().clone();
        observe_headers(client, issue, &headers);

        if is_redirect(status) {
            if hop == 5 {
                return Err(Error::UnexpectedRedirect);
            }
            let location = location_url(&url, &headers)?;
            if !client.same_origin(&location) {
                return Err(Error::CrossOrigin);
            }
            match status.as_u16() {
                303 => {
                    method = Method::GET;
                    body = None;
                }
                301 | 302 => {
                    if !original_was_get && method != Method::GET {
                        return Err(Error::UnexpectedRedirect);
                    }
                    // 301/302 → GET only when the original was GET.
                    if original_was_get {
                        method = Method::GET;
                        body = None;
                    } else {
                        return Err(Error::UnexpectedRedirect);
                    }
                }
                307 | 308 => {
                    // same method and body
                }
                _ => return Err(Error::UnexpectedRedirect),
            }
            url = location;
            continue;
        }

        let bytes = response.bytes().await.map_err(|e| map_reqwest_error(&e))?;
        return Ok((status, headers, bytes.to_vec()));
    }
    Err(Error::UnexpectedRedirect)
}

pub(crate) async fn execute_transfer(
    client: &Client,
    request: TransferRequest,
) -> Result<TransferResponse, Error> {
    if request.url.scheme() != "https" {
        return Err(Error::Network);
    }

    let mut attempt = 0_u32;
    loop {
        let result = execute_transfer_once(client, &request).await;
        match result {
            Ok(resp) if is_rate_limited(resp.status, &resp.body) && attempt < 4 => {
                let retry_after = parse_retry_after(&resp.headers);
                let _delay = retry_delays(attempt, retry_after, client.governor().jitter()).await;
                attempt += 1;
            }
            other => return other,
        }
    }
}

async fn execute_transfer_once(
    client: &Client,
    request: &TransferRequest,
) -> Result<TransferResponse, Error> {
    let permit = client
        .governor()
        .admit(Lane::Storage, &request.route_key)
        .await;
    let issue = permit.issue();

    match request.kind {
        TransferKind::Upload => {
            let mut builder = client
                .http()
                .request(request.method.clone(), request.url.clone());
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            // Token only on same-origin.
            if let Some(auth) = client.auth_header_for(&request.url) {
                builder = builder.header(reqwest::header::AUTHORIZATION, auth);
            }
            if let Some(ref bytes) = request.body {
                builder = builder.body(bytes.clone());
            }
            let response = builder.send().await.map_err(|e| map_reqwest_error(&e))?;
            let status = response.status();
            let headers = response.headers().clone();
            observe_headers(client, issue, &headers);
            if is_redirect(status) {
                // Upload POST is never auto-followed.
                return Err(Error::UploadIncomplete {
                    status: status.as_u16(),
                });
            }
            let body = response.bytes().await.map_err(|e| map_reqwest_error(&e))?;
            Ok(TransferResponse {
                status,
                headers,
                body: body.to_vec(),
                final_url: request.url.clone(),
            })
        }
        TransferKind::Download => {
            let mut url = request.url.clone();
            for hop in 0..=5 {
                if hop > 5 {
                    return Err(Error::UnexpectedRedirect);
                }
                if url.scheme() != "https" {
                    return Err(Error::Network);
                }
                let mut builder = client.http().request(Method::GET, url.clone());
                for (name, value) in &request.headers {
                    builder = builder.header(name, value);
                }
                // Accept-Encoding: identity for byte-count comparison (download path).
                builder = builder.header(
                    reqwest::header::ACCEPT_ENCODING,
                    HeaderValue::from_static("identity"),
                );
                if let Some(auth) = client.auth_header_for(&url) {
                    builder = builder.header(reqwest::header::AUTHORIZATION, auth);
                }
                let response = builder.send().await.map_err(|e| map_reqwest_error(&e))?;
                let status = response.status();
                let headers = response.headers().clone();
                observe_headers(client, issue, &headers);

                if is_redirect(status) {
                    if hop == 5 {
                        return Err(Error::UnexpectedRedirect);
                    }
                    let location = location_url(&url, &headers)?;
                    if location.scheme() != "https" {
                        return Err(Error::Network);
                    }
                    url = location;
                    continue;
                }

                let body = response.bytes().await.map_err(|e| map_reqwest_error(&e))?;
                return Ok(TransferResponse {
                    status,
                    headers,
                    body: body.to_vec(),
                    final_url: url,
                });
            }
            Err(Error::UnexpectedRedirect)
        }
    }
}

fn observe_headers(client: &Client, issue: u64, headers: &HeaderMap) {
    let remaining = header_f64(headers, "x-rate-limit-remaining");
    let cost = header_f64(headers, "x-request-cost");
    if let Some(remaining) = remaining {
        client.governor().observe(issue, remaining, cost);
    } else if let Some(cost) = cost {
        client.governor().observe_cost_only(issue, cost);
    }
}

fn header_f64(headers: &HeaderMap, name: &str) -> Option<f64> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

fn is_rate_limited(status: StatusCode, body: &[u8]) -> bool {
    if status.as_u16() == 429 {
        return true;
    }
    if status.as_u16() == 403 {
        let text = String::from_utf8_lossy(body);
        return text.contains("Rate Limit Exceeded");
    }
    false
}

fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    None
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

fn map_reqwest_error(err: &reqwest::Error) -> Error {
    if err.is_timeout() {
        Error::Timeout
    } else {
        Error::Network
    }
}

fn route_key_for(method: &Method, url: &Url) -> String {
    format!("{method} {}", url.path())
}

impl std::fmt::Debug for ApiRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiRequest")
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for TransferRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransferRequest")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
