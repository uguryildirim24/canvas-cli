//! Submission file upload transport (§11).

use std::fmt;
use std::time::Duration;

use futures_util::{StreamExt, stream};
use reqwest::header::{CONTENT_TYPE, HeaderValue, LOCATION};
use reqwest::{Body, Url};
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::{mpsc, watch};

use crate::governor::Lane;
use crate::request::{ApiRequest, map_reqwest_error, observe_headers, validate_transfer_url};
use crate::{Client, Error};

const IDLE: Duration = Duration::from_secs(60);
const CHUNK: usize = 64 * 1024;

/// Metadata for a Canvas submission file upload.
#[derive(Debug, Clone)]
pub struct UploadMeta {
    pub name: String,
    pub size: u64,
    pub content_type: String,
}

/// Successful upload result, hashed while sending.
#[derive(Debug, Clone)]
pub struct UploadResult {
    pub file_id: i64,
    pub sha256: [u8; 32],
}

#[derive(Deserialize)]
struct UploadSession {
    upload_url: String,
    #[serde(deserialize_with = "ordered_params")]
    upload_params: Vec<(String, String)>,
}

// Deserialize the wire map directly: serde_json::Map sorts without preserve_order.
fn ordered_params<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, String)>, D::Error> {
    struct Pairs;
    impl<'de> Visitor<'de> for Pairs {
        type Value = Vec<(String, String)>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an ordered map of upload parameters")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut pairs = Vec::new();
            while let Some(pair) = map.next_entry::<String, String>()? {
                pairs.push(pair);
            }
            Ok(pairs)
        }
    }
    d.deserialize_map(Pairs)
}

/// Upload with bounded memory, no replay, and a write-progress idle deadline.
pub async fn upload_submission_file(
    client: &Client,
    course_id: i64,
    assignment_id: i64,
    meta: &UploadMeta,
    body: impl AsyncRead + Unpin + Send,
) -> Result<UploadResult, Error> {
    let path =
        format!("/api/v1/courses/{course_id}/assignments/{assignment_id}/submissions/self/files");
    let session = json!({
        "name": meta.name, "size": meta.size, "content_type": meta.content_type,
    });
    upload_via(client, &path, &session, meta, body).await
}

/// Upload one file into the user's own files, for a conversation attachment.
///
/// Same §11 transport as a submission upload: a session request, a streamed
/// multipart body hashed as it is sent, and the completion handoff. Only the
/// session endpoint differs, and `parent_folder_path` names the folder Canvas
/// keeps conversation attachments in.
pub async fn upload_user_file(
    client: &Client,
    parent_folder_path: &str,
    meta: &UploadMeta,
    body: impl AsyncRead + Unpin + Send,
) -> Result<UploadResult, Error> {
    let session = json!({
        "name": meta.name,
        "size": meta.size,
        "content_type": meta.content_type,
        "parent_folder_path": parent_folder_path,
        // Never replace a file the user already has: a same-named upload gets
        // a new name instead of overwriting bytes this tool did not write.
        "on_duplicate": "rename",
    });
    upload_via(client, "/api/v1/users/self/files", &session, meta, body).await
}

async fn upload_via(
    client: &Client,
    path: &str,
    session_body: &serde_json::Value,
    meta: &UploadMeta,
    body: impl AsyncRead + Unpin + Send,
) -> Result<UploadResult, Error> {
    let url = client.origin().join(path).map_err(|_| Error::Network)?;
    let req = ApiRequest::new(reqwest::Method::POST, url).json(session_body)?;
    let session: UploadSession = client.send_api(req).await?;
    let upload_url = Url::parse(&session.upload_url).map_err(|_| Error::Network)?;
    validate_transfer_url(&upload_url)?;

    // Let reqwest encode disposition names/filenames and generate the boundary.
    let (tx, rx) = mpsc::channel::<Vec<u8>>(1);
    let (progress, mut observed) = watch::channel(0_u64);
    let stream = stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|bytes| (Ok::<_, std::io::Error>(bytes), rx))
    });
    let mut form = reqwest::multipart::Form::new();
    for (name, value) in session.upload_params {
        form = form.text(name, value);
    }
    let part = reqwest::multipart::Part::stream(Body::wrap_stream(stream))
        .file_name(meta.name.clone())
        .mime_str(&meta.content_type)
        .map_err(|_| Error::Decode)?;
    form = form.part("file", part);
    let content_type = HeaderValue::from_str(&format!(
        "multipart/form-data; boundary={}",
        form.boundary()
    ))
    .map_err(|_| Error::Decode)?;
    // Observe *all* multipart chunks as the HTTP stack requests them, including
    // the trailing boundary, so backpressure cannot mask a stalled socket.
    let multipart = form.into_stream().inspect(move |_| {
        progress.send_modify(|n| *n += 1);
    });
    let permit = client
        .governor()
        .admit(Lane::Storage, "transfer:upload")
        .await;
    let send = client
        .upload_http()
        .post(upload_url.clone())
        .header(CONTENT_TYPE, content_type)
        .body(Body::wrap_stream(multipart))
        .send();
    let send = async {
        tokio::pin!(send);
        loop {
            tokio::select! {
                result = &mut send => break result.map_err(|e| map_reqwest_error(&e)),
                change = tokio::time::timeout(IDLE, observed.changed()) => {
                    match change {
                        Err(_) => break Err(Error::Timeout),
                        Ok(Ok(())) => {},
                        Ok(Err(_)) => break tokio::time::timeout(IDLE, &mut send).await
                            .map_err(|_| Error::Timeout)?.map_err(|e| map_reqwest_error(&e)),
                    }
                }
            }
        }
    };
    // Neither future is spawned: cancellation drops the producer and socket together.
    let (sha256, response) = tokio::try_join!(produce(body, meta.size, tx), send)?;
    let status = response.status();
    let headers = response.headers().clone();
    observe_headers(client, permit.issue(), &headers);
    let bytes = tokio::time::timeout(IDLE, response.bytes())
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|e| map_reqwest_error(&e))?;
    drop(permit); // Finalization is a separate API request.
    let resp = crate::TransferResponse {
        status,
        headers,
        body: bytes.to_vec(),
        final_url: upload_url,
    };
    let file_id = complete_upload(client, &resp).await?;
    Ok(UploadResult { file_id, sha256 })
}

async fn produce(
    mut body: impl AsyncRead + Unpin,
    expected: u64,
    tx: mpsc::Sender<Vec<u8>>,
) -> Result<[u8; 32], Error> {
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    loop {
        let mut chunk = vec![0; CHUNK];
        let n = tokio::time::timeout(IDLE, body.read(&mut chunk))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Network)?;
        if n == 0 {
            if total != expected {
                return Err(Error::SizeMismatch);
            }
            return Ok(hasher.finalize().into());
        }
        total = total.checked_add(n as u64).ok_or(Error::SizeMismatch)?;
        if total > expected {
            return Err(Error::SizeMismatch);
        }
        chunk.truncate(n);
        hasher.update(&chunk);
        tokio::time::timeout(IDLE, tx.send(chunk))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Network)?;
    }
}

async fn complete_upload(client: &Client, resp: &crate::TransferResponse) -> Result<i64, Error> {
    let status = resp.status.as_u16();
    if status == 201 {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&resp.body)
            && let Some(id) = json_id(&v)
        {
            return Ok(id);
        }
    } else if !crate::request::is_redirect(resp.status) {
        return Err(Error::UploadIncomplete { status });
    }
    let location = resp
        .headers
        .get(LOCATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| resp.final_url.join(v).ok())
        .filter(|u| client.same_origin(u))
        .ok_or(Error::UploadIncomplete { status })?;
    let value: serde_json::Value = client
        .send_api(ApiRequest::new(reqwest::Method::GET, location))
        .await?;
    json_id(&value).ok_or(Error::UploadIncomplete { status })
}

fn json_id(value: &serde_json::Value) -> Option<i64> {
    value
        .get("id")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .filter(|id| *id > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn source_idle_timeout_and_bounded_backpressure() {
        let (reader, _writer) = tokio::io::duplex(1);
        let (tx, _rx) = mpsc::channel(1);
        assert!(matches!(produce(reader, 1, tx).await, Err(Error::Timeout)));
        let (tx, _rx) = mpsc::channel(1);
        assert!(matches!(
            produce(&vec![1; CHUNK * 3][..], (CHUNK * 3) as u64, tx).await,
            Err(Error::Timeout)
        ));
    }
    #[tokio::test(start_paused = true)]
    async fn progress_can_continue_beyond_sixty_seconds() {
        use tokio::io::AsyncWriteExt;
        let (reader, mut writer) = tokio::io::duplex(1);
        let (tx, mut rx) = mpsc::channel(1);
        let write = async {
            for _ in 0..3 {
                tokio::time::sleep(Duration::from_secs(40)).await;
                writer.write_all(b"x").await.unwrap();
            }
            drop(writer);
        };
        let drain = async { while rx.recv().await.is_some() {} };
        let (result, (), ()) = tokio::join!(produce(reader, 3, tx), write, drain);
        assert_eq!(
            result.unwrap().as_slice(),
            Sha256::digest(b"xxx").as_slice()
        );
    }
    #[tokio::test]
    async fn source_size_errors_do_not_return_a_digest() {
        for expected in [0, 2] {
            let (tx, mut rx) = mpsc::channel(1);
            let (result, ()) = tokio::join!(produce(&b"x"[..], expected, tx), async {
                while rx.recv().await.is_some() {}
            });
            assert!(matches!(result, Err(Error::SizeMismatch)));
        }
    }
}
