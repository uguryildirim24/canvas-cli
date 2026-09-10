//! Canvas LMS HTTP client, secrets, and API models.

pub mod download;
pub mod error;
pub mod governor;
pub mod models;
pub mod redact;
pub mod request;
pub mod serde_util;
pub mod submission;
pub mod upload;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use futures_util::Stream;
use futures_util::stream::{self, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::redirect::Policy;
use reqwest::{Client as HttpClient, Method, StatusCode, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use error::Error;
pub use governor::{Governor, GovernorConfig, Lane, Telemetry};
pub use models::{Page, WrappedCollection};
pub use redact::RedactingLayer;
pub use request::{ApiRequest, TransferKind, TransferRequest};
pub use serde_util::Supplied;
pub use submission::{
    SubmissionBody, get_assignment_for_submit, get_submission_history, is_canvas_error_body,
    post_submission, sanitize_post_error_text,
};

/// An API token that never prints its value.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// Wrap a raw token string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the raw token for authenticated requests.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

/// HTTP client for Canvas API and transfer requests.
#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    origin: Url,
    token: Secret,
    user_agent: String,
    http: HttpClient,
    transfer_http: HttpClient,
    upload_http: HttpClient,
    governor: Governor,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("origin", &self.inner.origin)
            .field("token", &self.inner.token)
            .field("user_agent", &self.inner.user_agent)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Build a Canvas API client.
    pub fn new(origin: Url, token: Secret, user_agent: &str) -> Result<Self, Error> {
        Self::with_governor(origin, token, user_agent, GovernorConfig::default())
    }

    /// Build a client with an explicit governor configuration (tests).
    pub fn with_governor(
        origin: Url,
        token: Secret,
        user_agent: &str,
        mut governor: GovernorConfig,
    ) -> Result<Self, Error> {
        if !matches!(origin.scheme(), "http" | "https")
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.path() != "/"
        {
            return Err(Error::CrossOrigin);
        }
        HeaderValue::from_str(&format!("Bearer {}", token.expose()))
            .map_err(|_| Error::Unauthorized)?;
        governor.api_concurrency = governor.api_concurrency.clamp(1, 8);
        let http = HttpClient::builder()
            .use_rustls_tls()
            .no_proxy()
            .gzip(true)
            .brotli(true)
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(user_agent)
            .build()
            .map_err(|_| Error::Network)?;

        // Transfer lane: connect 10s, idle-read 60s, no total timeout; no auto decompress.
        let transfer_http = HttpClient::builder()
            .use_rustls_tls()
            .no_proxy()
            .no_gzip()
            .no_brotli()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .user_agent(user_agent)
            .build()
            .map_err(|_| Error::Network)?;

        let upload_http = HttpClient::builder()
            .use_rustls_tls()
            .no_proxy()
            .no_gzip()
            .no_brotli()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .user_agent(user_agent)
            .build()
            .map_err(|_| Error::Network)?;
        Ok(Self {
            inner: Arc::new(ClientInner {
                origin,
                token,
                user_agent: user_agent.to_owned(),
                http,
                transfer_http,
                upload_http,
                governor: Governor::new(governor),
            }),
        })
    }

    /// Canonical Canvas origin.
    pub fn origin(&self) -> &Url {
        &self.inner.origin
    }

    /// Redacted token handle.
    pub fn token(&self) -> &Secret {
        &self.inner.token
    }

    /// User-Agent header value.
    pub fn user_agent(&self) -> &str {
        &self.inner.user_agent
    }

    /// Shared rate-limit governor.
    pub fn governor(&self) -> &Governor {
        &self.inner.governor
    }

    /// Current request telemetry counters.
    pub fn telemetry(&self) -> Telemetry {
        self.inner.governor.telemetry()
    }

    /// GET a single JSON resource.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        let url = self.api_url(path)?;
        let req = ApiRequest::new(Method::GET, url);
        self.send_api(req).await
    }

    /// GET every page of a JSON array collection (`per_page=100`).
    pub fn get_all<T: DeserializeOwned + Send + 'static>(
        &self,
        path: &str,
    ) -> impl Stream<Item = Result<Page<T>, Error>> + Send + '_ {
        let client = self.clone();
        let start = self.api_url_with_per_page(path);
        stream::unfold(Some(start), move |state| {
            let client = client.clone();
            async move {
                let url = match state? {
                    Ok(url) => url,
                    Err(err) => return Some((Err(err), None)),
                };
                match client
                    .send_api_page::<T>(ApiRequest::new(Method::GET, url))
                    .await
                {
                    Ok(page) => {
                        let next = page.next.clone().map(Ok);
                        Some((Ok(page), next))
                    }
                    Err(err) => Some((Err(err), None)),
                }
            }
        })
    }

    /// GET every page of a wrapped collection such as grading periods.
    pub fn get_all_wrapped<T: DeserializeOwned + Send + 'static>(
        &self,
        path: &str,
    ) -> impl Stream<Item = Result<Page<T>, Error>> + Send + '_ {
        let client = self.clone();
        let start = self.api_url_with_per_page(path);
        stream::unfold(Some(start), move |state| {
            let client = client.clone();
            async move {
                let url = match state? {
                    Ok(url) => url,
                    Err(err) => return Some((Err(err), None)),
                };
                match client
                    .send_api_wrapped_page::<T>(ApiRequest::new(Method::GET, url))
                    .await
                {
                    Ok(page) => {
                        let next = page.next.clone().map(Ok);
                        Some((Ok(page), next))
                    }
                    Err(err) => Some((Err(err), None)),
                }
            }
        })
    }

    /// POST JSON and decode the response body.
    pub async fn post<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, Error> {
        let url = self.api_url(path)?;
        let req = ApiRequest::new(Method::POST, url).json(body)?;
        self.send_api(req).await
    }

    /// PUT JSON and decode the response body.
    pub async fn put<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, Error> {
        let url = self.api_url(path)?;
        let req = ApiRequest::new(Method::PUT, url).json(body)?;
        self.send_api(req).await
    }

    /// DELETE a resource and decode the response body.
    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        let url = self.api_url(path)?;
        let req = ApiRequest::new(Method::DELETE, url);
        self.send_api(req).await
    }

    /// Run a transfer-phase request (upload POST or download GET hops).
    pub async fn send_transfer(&self, request: TransferRequest) -> Result<TransferResponse, Error> {
        request::execute_transfer(self, request).await
    }

    /// Execute an API request and return the raw status, headers, body, and final URL.
    ///
    /// Non-2xx responses are **not** mapped to [`Error`]. Rate-limit responses (429 and
    /// Canvas' 403 rate-limit body) still retry via the governor, then surface as
    /// [`Error::RateLimited`] when retries are exhausted.
    pub async fn execute_api(
        &self,
        request: ApiRequest,
    ) -> Result<(StatusCode, HeaderMap, Vec<u8>, Url), Error> {
        request::execute_api(self, request).await
    }

    /// Join `path` (may include a query string) onto the Canvas origin.
    pub fn api_url(&self, path: &str) -> Result<Url, Error> {
        self.inner.origin.join(path).map_err(|_| Error::Network)
    }

    /// Collect all pages from [`Self::get_all`].
    pub async fn get_all_vec<T: DeserializeOwned + Send + 'static>(
        &self,
        path: &str,
    ) -> Result<Vec<T>, Error> {
        let mut out = Vec::new();
        let mut pages = std::pin::pin!(self.get_all::<T>(path));
        while let Some(page) = pages.next().await {
            out.extend(page?.items);
        }
        Ok(out)
    }

    pub(crate) fn http(&self) -> &HttpClient {
        &self.inner.http
    }

    pub(crate) fn upload_http(&self) -> &HttpClient {
        &self.inner.upload_http
    }

    pub(crate) fn transfer_http(&self) -> &HttpClient {
        &self.inner.transfer_http
    }

    pub(crate) fn same_origin(&self, url: &Url) -> bool {
        url.username().is_empty()
            && url.password().is_none()
            && url.scheme() == self.inner.origin.scheme()
            && url.host_str() == self.inner.origin.host_str()
            && url.port_or_known_default() == self.inner.origin.port_or_known_default()
    }

    fn api_url_with_per_page(&self, path: &str) -> Result<Url, Error> {
        let mut url = self.api_url(path)?;
        let has_per_page = url.query_pairs().any(|(k, _)| k == "per_page");
        if !has_per_page {
            url.query_pairs_mut().append_pair("per_page", "100");
        }
        Ok(url)
    }

    /// Execute an API-phase builder with the same origin and redirect checks.
    pub async fn send_api<T: DeserializeOwned>(&self, request: ApiRequest) -> Result<T, Error> {
        self.send_api_with_headers(request)
            .await
            .map(|(value, _)| value)
    }

    /// Execute an API request and retain response headers for diagnostics.
    pub async fn send_api_with_headers<T: DeserializeOwned>(
        &self,
        request: ApiRequest,
    ) -> Result<(T, HeaderMap), Error> {
        let (status, headers, bytes, _url) = request::execute_api(self, request).await?;
        if status.is_success() {
            let bytes = if status == StatusCode::NO_CONTENT {
                b"null".as_slice()
            } else {
                &bytes
            };
            return serde_util::with_origin(self.origin(), || {
                serde_json::from_slice(bytes).map_err(|_| Error::Decode)
            })
            .map(|value| (value, headers));
        }
        Err(classify_status(status, &bytes))
    }

    pub(crate) async fn send_api_page<T: DeserializeOwned>(
        &self,
        request: ApiRequest,
    ) -> Result<Page<T>, Error> {
        let (status, headers, bytes, url) = request::execute_api(self, request).await?;
        if !status.is_success() {
            return Err(classify_status(status, &bytes));
        }
        let items: Vec<T> = serde_util::with_origin(self.origin(), || {
            serde_json::from_slice(&bytes).map_err(|_| Error::Decode)
        })?;
        let next = link_next(&headers, &url)?;
        if let Some(ref next_url) = next
            && !self.same_origin(next_url)
        {
            return Err(Error::CrossOrigin);
        }
        Ok(Page { items, next })
    }

    pub(crate) async fn send_api_wrapped_page<T: DeserializeOwned>(
        &self,
        request: ApiRequest,
    ) -> Result<Page<T>, Error> {
        let (status, headers, bytes, url) = request::execute_api(self, request).await?;
        if !status.is_success() {
            return Err(classify_status(status, &bytes));
        }
        let wrapped: WrappedCollection<T> = serde_util::with_origin(self.origin(), || {
            serde_json::from_slice(&bytes).map_err(|_| Error::Decode)
        })?;
        let next = link_next(&headers, &url)?;
        if let Some(ref next_url) = next
            && !self.same_origin(next_url)
        {
            return Err(Error::CrossOrigin);
        }
        Ok(Page {
            items: wrapped.into_items(),
            next,
        })
    }

    pub(crate) fn auth_header_for(&self, url: &Url) -> Option<HeaderValue> {
        if self.same_origin(url) {
            let value = format!("Bearer {}", self.inner.token.expose());
            HeaderValue::from_str(&value).ok().map(|mut header| {
                header.set_sensitive(true);
                header
            })
        } else {
            None
        }
    }
}

/// Raw transfer response after phase rules are applied.
pub struct TransferResponse {
    /// Final HTTP status.
    pub status: StatusCode,
    /// Response headers of the final hop (or upload POST response).
    pub headers: HeaderMap,
    /// Response body bytes.
    pub body: Vec<u8>,
    /// Final URL after download hops (upload POSTs keep the upload URL).
    pub final_url: Url,
}

fn classify_status(status: StatusCode, bytes: &[u8]) -> Error {
    let body = String::from_utf8_lossy(bytes).into_owned();
    match status.as_u16() {
        401 => Error::Unauthorized,
        404 => Error::NotFound,
        429 => Error::RateLimited,
        403 if body.contains("Rate Limit Exceeded") => Error::Forbidden {
            rate_limited: true,
            body,
        },
        403 => Error::Forbidden {
            rate_limited: false,
            body,
        },
        code @ 400..=499 => {
            if let Some(errors) = parse_validation_errors(bytes) {
                Error::Validation {
                    status: code,
                    errors,
                }
            } else {
                Error::Denied { status: code }
            }
        }
        _ => Error::Denied {
            status: status.as_u16(),
        },
    }
}

pub(crate) fn parse_validation_errors(bytes: &[u8]) -> Option<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct Envelope {
        errors: Option<serde_json::Value>,
    }
    let env: Envelope = serde_json::from_slice(bytes).ok()?;
    let errors = env.errors?;
    Some(flatten_errors(&errors))
}

fn flatten_errors(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(items) => items
            .iter()
            .flat_map(|item| match item {
                serde_json::Value::String(s) => vec![s.clone()],
                serde_json::Value::Object(map) => map
                    .get("message")
                    .and_then(|m| m.as_str())
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
                other => vec![other.to_string()],
            })
            .collect(),
        serde_json::Value::Object(map) => map
            .iter()
            .flat_map(|(k, v)| match v {
                serde_json::Value::Array(arr) => arr
                    .iter()
                    .map(|item| match item {
                        serde_json::Value::Object(o) => {
                            if let Some(m) = o.get("message").and_then(|m| m.as_str()) {
                                format!("{k}: {m}")
                            } else {
                                format!("{k}: {item}")
                            }
                        }
                        other => format!("{k}: {other}"),
                    })
                    .collect::<Vec<_>>(),
                other => vec![format!("{k}: {other}")],
            })
            .collect(),
        other => vec![other.to_string()],
    }
}

fn link_next(headers: &HeaderMap, current: &Url) -> Result<Option<Url>, Error> {
    for value in headers.get_all(reqwest::header::LINK) {
        let value = value.to_str().map_err(|_| Error::Decode)?;
        for part in split_link_values(value, ',')? {
            let part = part.trim();
            let (target, params) = part
                .strip_prefix('<')
                .and_then(|s| s.split_once('>'))
                .ok_or(Error::Decode)?;
            let is_next = split_link_values(params, ';')?.into_iter().any(|param| {
                let Some((name, value)) = param.trim().split_once('=') else {
                    return false;
                };
                name.trim().eq_ignore_ascii_case("rel")
                    && value
                        .trim()
                        .trim_matches('"')
                        .split_ascii_whitespace()
                        .any(|rel| rel.eq_ignore_ascii_case("next"))
            });
            if is_next {
                return current.join(target).map(Some).map_err(|_| Error::Decode);
            }
        }
    }
    Ok(None)
}

fn split_link_values(input: &str, delimiter: char) -> Result<Vec<&str>, Error> {
    let mut parts = Vec::new();
    let (mut angle, mut quoted, mut escaped, mut start) = (false, false, false, 0);
    for (i, c) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => escaped = true,
            '"' if !angle => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            c if c == delimiter && !angle && !quoted => {
                parts.push(&input[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if angle || quoted {
        return Err(Error::Decode);
    }
    parts.push(&input[start..]);
    Ok(parts)
}

impl fmt::Debug for TransferResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransferResponse")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod transfer_tests;
