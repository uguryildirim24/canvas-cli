//! Submission file upload transport (§11).

use std::io::Write as _;

use reqwest::Url;
use reqwest::header::{CONTENT_TYPE, HeaderName, HeaderValue, LOCATION};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::request::{ApiRequest, TransferRequest};
use crate::{Client, Error};

/// Metadata for a Canvas submission file upload.
#[derive(Debug, Clone)]
pub struct UploadMeta {
    /// File display name.
    pub name: String,
    /// Declared size in bytes.
    pub size: u64,
    /// MIME type.
    pub content_type: String,
}

/// Successful upload result.
#[derive(Debug, Clone)]
pub struct UploadResult {
    /// Canvas file id.
    pub file_id: i64,
    /// SHA-256 of the streamed body bytes.
    pub sha256: [u8; 32],
}

#[derive(Debug, Deserialize)]
struct UploadSession {
    upload_url: String,
    #[serde(default)]
    upload_params: serde_json::Map<String, serde_json::Value>,
}

/// Upload one submission file exactly per §11.
///
/// Returns the Canvas file id and the streamed SHA-256 of `body`.
pub async fn upload_submission_file(
    client: &Client,
    course_id: i64,
    assignment_id: i64,
    meta: &UploadMeta,
    mut body: impl AsyncRead + Unpin + Send,
) -> Result<UploadResult, Error> {
    let path = format!(
        "/api/v1/courses/{course_id}/assignments/{assignment_id}/submissions/self/files"
    );
    let url = client
        .origin()
        .join(&path)
        .map_err(|_| Error::Network)?;
    // Canvas accepts JSON or form; JSON keeps the API phase simple.
    let req = ApiRequest::new(reqwest::Method::POST, url).json(&json!({
        "name": meta.name,
        "size": meta.size,
        "content_type": meta.content_type,
    }))?;
    let session: UploadSession = client.send_api(req).await?;

    let upload_url = Url::parse(&session.upload_url).map_err(|_| Error::Network)?;
    if !matches!(upload_url.scheme(), "http" | "https") {
        return Err(Error::Network);
    }

    let mut hasher = Sha256::new();
    let mut file_bytes = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = body.read(&mut buf).await.map_err(|_| Error::Network)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file_bytes.extend_from_slice(&buf[..n]);
    }
    if file_bytes.len() as u64 != meta.size {
        return Err(Error::SizeMismatch);
    }
    let sha256: [u8; 32] = hasher.finalize().into();

    let boundary = format!("----canvas-cli-{}", hex_short(&sha256));
    let multipart = build_multipart(&boundary, &session.upload_params, meta, &file_bytes)?;
    let mut transfer = TransferRequest::upload(upload_url);
    transfer = transfer.header(
        CONTENT_TYPE,
        HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
            .map_err(|_| Error::Decode)?,
    );
    transfer = transfer.body(multipart);

    let resp = client.send_transfer(transfer).await?;
    let file_id = complete_upload(client, &resp).await?;
    Ok(UploadResult { file_id, sha256 })
}

async fn complete_upload(
    client: &Client,
    resp: &crate::TransferResponse,
) -> Result<i64, Error> {
    let status = resp.status.as_u16();
    if matches!(status, 301 | 302 | 303 | 307 | 308) {
        let location = location_from(&resp.headers, &resp.final_url)?;
        if !client.same_origin(&location) {
            return Err(Error::UploadIncomplete { status });
        }
        return fetch_file_id(client, location).await;
    }
    if status == 201 {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&resp.body)
            && let Some(id) = json_id(&v)
        {
            return Ok(id);
        }
        let location = match location_from(&resp.headers, &resp.final_url) {
            Ok(u) => u,
            Err(_) => return Err(Error::UploadIncomplete { status }),
        };
        if !client.same_origin(&location) {
            return Err(Error::UploadIncomplete { status });
        }
        return fetch_file_id(client, location).await;
    }
    Err(Error::UploadIncomplete { status })
}

async fn fetch_file_id(client: &Client, location: Url) -> Result<i64, Error> {
    let req = ApiRequest::new(reqwest::Method::GET, location);
    let value: serde_json::Value = client.send_api(req).await?;
    json_id(&value).ok_or(Error::Decode)
}

fn json_id(value: &serde_json::Value) -> Option<i64> {
    value.get("id").and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_u64().map(|u| u as i64))
            .or_else(|| v.as_str()?.parse().ok())
    })
}

fn location_from(headers: &reqwest::header::HeaderMap, base: &Url) -> Result<Url, Error> {
    let raw = headers
        .get(LOCATION)
        .ok_or(Error::UploadIncomplete {
            status: 0,
        })?
        .to_str()
        .map_err(|_| Error::UploadIncomplete { status: 0 })?;
    base.join(raw)
        .or_else(|_| Url::parse(raw))
        .map_err(|_| Error::UploadIncomplete { status: 0 })
}

fn build_multipart(
    boundary: &str,
    params: &serde_json::Map<String, serde_json::Value>,
    meta: &UploadMeta,
    file_bytes: &[u8],
) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    for (key, value) in params {
        let text = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string().trim_matches('"').to_string(),
        };
        write!(
            out,
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{text}\r\n"
        )
        .map_err(|_| Error::Decode)?;
    }
    write!(
        out,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
        meta.name.replace('"', ""),
        meta.content_type
    )
    .map_err(|_| Error::Decode)?;
    out.extend_from_slice(file_bytes);
    write!(out, "\r\n--{boundary}--\r\n").map_err(|_| Error::Decode)?;
    let _ = HeaderName::from_static("content-type"); // keep import used if clippy
    Ok(out)
}

fn hex_short(bytes: &[u8; 32]) -> String {
    bytes[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
