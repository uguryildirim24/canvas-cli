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

pub(crate) type ApiResponse = (StatusCode, HeaderMap, Vec<u8>, Url);

pub(crate) async fn execute_api(
    client: &Client,
    request: ApiRequest,
) -> Result<ApiResponse, Error> {
    for attempt in 0..=4 {
        let (status, headers, body, url) = execute_api_once(client, &request).await?;
        if !is_rate_limited(status, &body) {
            return Ok((status, headers, body, url));
        }
        if attempt == 4 {
            return Err(Error::RateLimited);
        }
        retry_delays(
            attempt,
            parse_retry_after(&headers),
            client.governor().jitter(),
        )
        .await;
    }
    unreachable!("bounded retries return on the final attempt")
}

async fn execute_api_once(client: &Client, request: &ApiRequest) -> Result<ApiResponse, Error> {
    let mut method = request.method.clone();
    let mut url = request.url.clone();
    let mut body = request.body.clone();
    for hop in 0..=5 {
        if !client.same_origin(&url) {
            return Err(Error::CrossOrigin);
        }
        // Each hop is an actual request with its own issue number and cost.
        let route = if hop == 0 {
            request.route_key.clone()
        } else {
            route_key_for(&method, &url)
        };
        let permit = client.governor().admit(Lane::Api, &route).await;
        let mut builder = client.http().request(method.clone(), url.clone());
        if body.is_some() {
            builder = builder.headers(request.headers.clone());
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
        observe_headers(client, permit.issue(), &headers);
        if is_redirect(status) {
            let location = location_url(&url, &headers)?;
            if !client.same_origin(&location) {
                return Err(Error::CrossOrigin);
            }
            if hop == 5 {
                return Err(Error::UnexpectedRedirect);
            }
            match status.as_u16() {
                303 => {
                    method = Method::GET;
                    body = None;
                }
                301 | 302 if request.method == Method::GET => {
                    method = Method::GET;
                    body = None;
                }
                307 | 308 => {}
                _ => return Err(Error::UnexpectedRedirect),
            }
            url = location;
        } else if status.is_redirection() {
            return Err(Error::UnexpectedRedirect);
        } else {
            let bytes = response.bytes().await.map_err(|e| map_reqwest_error(&e))?;
            return Ok((status, headers, bytes.to_vec(), url));
        }
    }
    Err(Error::UnexpectedRedirect)
}

pub(crate) async fn execute_transfer(
    client: &Client,
    request: TransferRequest,
) -> Result<TransferResponse, Error> {
    validate_transfer_url(&request.url)?;
    for attempt in 0..=4 {
        let response = execute_transfer_once(client, &request).await?;
        if is_rate_limited(response.status, &response.body) {
            // The upload stream must never be replayed. Its caller owns recovery.
            if request.kind == TransferKind::Upload || attempt == 4 {
                return Err(Error::RateLimited);
            }
            retry_delays(
                attempt,
                parse_retry_after(&response.headers),
                client.governor().jitter(),
            )
            .await;
            continue;
        }
        if request.kind == TransferKind::Download && !response.status.is_success() {
            if client.same_origin(&response.final_url) {
                return Err(Error::Denied {
                    status: response.status.as_u16(),
                });
            }
            if response.status == StatusCode::FORBIDDEN {
                return Err(Error::StorageExpired);
            }
            return Err(Error::Denied {
                status: response.status.as_u16(),
            });
        }
        return Ok(response);
    }
    unreachable!("bounded retries return on the final attempt")
}

async fn execute_transfer_once(
    client: &Client,
    request: &TransferRequest,
) -> Result<TransferResponse, Error> {
    let mut url = request.url.clone();
    for hop in 0..=5 {
        validate_transfer_url(&url)?;
        let permit = client
            .governor()
            .admit(Lane::Storage, &request.route_key)
            .await;
        let headers = transfer_headers(client, request, &url);
        let mut builder = client
            .transfer_http()
            .request(request.method.clone(), url.clone())
            .headers(headers);
        if request.kind == TransferKind::Upload
            && let Some(ref bytes) = request.body
        {
            builder = builder.body(bytes.clone());
        }
        let response = builder.send().await.map_err(|e| map_reqwest_error(&e))?;
        let status = response.status();
        let headers = response.headers().clone();
        observe_headers(client, permit.issue(), &headers);
        if request.kind == TransferKind::Upload {
            // Surface the validated completion handoff without following or replaying.
            validate_upload_handoff(client, &url, status, &headers)?;
        } else if is_redirect(status) {
            if hop == 5 {
                return Err(Error::UnexpectedRedirect);
            }
            url = location_url(&url, &headers)?;
            continue;
        } else if status.is_redirection() {
            return Err(Error::UnexpectedRedirect);
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

pub(crate) fn validate_transfer_url(url: &Url) -> Result<(), Error> {
    let scheme_ok = url.scheme() == "https"
        || (url.scheme() == "http"
            && cfg!(debug_assertions)
            && matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
            && std::env::var("CANVAS_TEST_ALLOW_HTTP").ok().as_deref() == Some("1"));
    if !scheme_ok || !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Network);
    }
    Ok(())
}

fn transfer_headers(client: &Client, request: &TransferRequest, url: &Url) -> HeaderMap {
    let mut headers = request.headers.clone();
    // Caller-supplied credentials must not bypass phase rules or survive hops.
    headers.remove(reqwest::header::AUTHORIZATION);
    headers.remove(reqwest::header::PROXY_AUTHORIZATION);
    headers.remove(reqwest::header::COOKIE);
    headers.remove(reqwest::header::HOST);
    if request.kind == TransferKind::Download {
        headers.insert(
            reqwest::header::ACCEPT_ENCODING,
            HeaderValue::from_static("identity"),
        );
        if let Some(auth) = client.auth_header_for(url) {
            headers.insert(reqwest::header::AUTHORIZATION, auth);
        }
    }
    headers
}

pub(crate) fn validate_upload_handoff(
    client: &Client,
    current: &Url,
    status: StatusCode,
    headers: &HeaderMap,
) -> Result<(), Error> {
    if status.is_redirection() {
        if !is_redirect(status) {
            return Err(Error::UploadIncomplete {
                status: status.as_u16(),
            });
        }
        let location = location_url(current, headers).map_err(|_| Error::UploadIncomplete {
            status: status.as_u16(),
        })?;
        if !client.same_origin(&location) {
            return Err(Error::UploadIncomplete {
                status: status.as_u16(),
            });
        }
    }
    Ok(())
}

pub(crate) fn observe_headers(client: &Client, issue: u64, headers: &HeaderMap) {
    let remaining = header_f64(headers, "x-rate-limit-remaining");
    let cost = header_f64(headers, "x-request-cost");
    if let Some(cost) = cost {
        tracing::debug!(cost, "Canvas request cost");
    }
    if let Some(remaining) = remaining {
        client.governor().observe(issue, remaining, cost);
    } else if let Some(cost) = cost {
        client.governor().observe_cost_only(issue, cost);
    }
}

fn header_f64(headers: &HeaderMap, name: &str) -> Option<f64> {
    let value: f64 = headers.get(name)?.to_str().ok()?.parse().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

pub(crate) fn is_redirect(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

pub(crate) fn is_rate_limited(status: StatusCode, body: &[u8]) -> bool {
    if status.as_u16() == 429 {
        return true;
    }
    if status.as_u16() == 403 {
        let text = String::from_utf8_lossy(body);
        return text.contains("Rate Limit Exceeded");
    }
    false
}

pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    Some(
        date.duration_since(std::time::SystemTime::now())
            .unwrap_or_default(),
    )
}

pub(crate) fn location_url(current: &Url, headers: &HeaderMap) -> Result<Url, Error> {
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

pub(crate) fn map_reqwest_error(err: &reqwest::Error) -> Error {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Secret;

    fn client() -> Client {
        Client::new(
            Url::parse("https://canvas.test").unwrap(),
            Secret::new("SECRET"),
            "test",
        )
        .unwrap()
    }

    #[test]
    fn transfer_credentials_are_recomputed_for_each_hop() {
        let client = client();
        let own = Url::parse("https://canvas.test/file").unwrap();
        let other = Url::parse("https://storage.test/file").unwrap();
        let request = TransferRequest::download(own.clone())
            .header(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_static("INJECTED"),
            )
            .header(reqwest::header::COOKIE, HeaderValue::from_static("SECRET"));
        let initial = transfer_headers(&client, &request, &own);
        assert_eq!(initial[reqwest::header::AUTHORIZATION], "Bearer SECRET");
        assert!(initial[reqwest::header::AUTHORIZATION].is_sensitive());
        assert_eq!(initial[reqwest::header::ACCEPT_ENCODING], "identity");
        let redirected = transfer_headers(&client, &request, &other);
        assert!(!redirected.contains_key(reqwest::header::AUTHORIZATION));
        assert!(!redirected.contains_key(reqwest::header::COOKIE));
        for url in [own, other] {
            let upload = TransferRequest::upload(url.clone()).header(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_static("INJECTED"),
            );
            assert!(
                !transfer_headers(&client, &upload, &url)
                    .contains_key(reqwest::header::AUTHORIZATION)
            );
        }
    }

    #[test]
    fn transfer_urls_and_upload_handoffs_enforce_phase_rules() {
        let client = client();
        let upload = Url::parse("https://storage.test/upload").unwrap();
        for bad in [
            "http://storage.test/file",
            "https://user:SECRET@storage.test/file",
        ] {
            assert!(validate_transfer_url(&Url::parse(bad).unwrap()).is_err());
        }
        let mut headers = HeaderMap::new();
        assert!(matches!(
            validate_upload_handoff(&client, &upload, StatusCode::SEE_OTHER, &headers),
            Err(Error::UploadIncomplete { status: 303 })
        ));
        headers.insert(
            LOCATION,
            HeaderValue::from_static("https://canvas.test/api/v1/files/1"),
        );
        assert!(validate_upload_handoff(&client, &upload, StatusCode::SEE_OTHER, &headers).is_ok());
        assert!(matches!(
            validate_upload_handoff(&client, &upload, StatusCode::NOT_MODIFIED, &headers),
            Err(Error::UploadIncomplete { status: 304 })
        ));
        headers.insert(
            LOCATION,
            HeaderValue::from_static("https://other.test/file"),
        );
        assert!(matches!(
            validate_upload_handoff(&client, &upload, StatusCode::SEE_OTHER, &headers),
            Err(Error::UploadIncomplete { status: 303 })
        ));
        assert!(
            validate_upload_handoff(&client, &upload, StatusCode::CREATED, &HeaderMap::new())
                .is_ok()
        );
    }

    #[test]
    fn retry_after_accepts_http_dates_and_ignores_invalid_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            HeaderValue::from_static("Thu, 01 Jan 1970 00:00:00 GMT"),
        );
        assert_eq!(parse_retry_after(&headers), Some(Duration::ZERO));
        let date = httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(30));
        headers.insert(
            reqwest::header::RETRY_AFTER,
            HeaderValue::from_str(&date).unwrap(),
        );
        let delay = parse_retry_after(&headers).unwrap();
        assert!(delay > Duration::from_secs(28) && delay <= Duration::from_secs(30));
        headers.insert(
            reqwest::header::RETRY_AFTER,
            HeaderValue::from_static("invalid"),
        );
        assert_eq!(parse_retry_after(&headers), None);
    }
}
