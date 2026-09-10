//! Wire `canvas-api::download` into the install [`Transfer`] trait.

use std::sync::Arc;
use tokio::io::AsyncWrite;

use canvas_api::download::download as api_download;
use canvas_api::models::File;
use canvas_api::{Client, Error as ApiError};

use crate::download::install::{RemoteMeta, Transfer, TransferError};

/// Progress callback: `(file_id, cumulative_bytes)`.
pub type ProgressFn = Arc<dyn Fn(i64, u64) + Send + Sync>;

/// Fresh file metadata for install planning. Capability URLs stay in memory only.
#[derive(Clone)]
pub struct FileFetchMeta {
    pub remote: RemoteMeta,
    pub locked: bool,
    file_id: i64,
    url: Option<reqwest::Url>,
}

/// `GET /api/v1/files/:id` → fresh metadata for classification and transfer.
pub async fn file_remote_meta(
    client: &Client,
    file_id: i64,
) -> Result<FileFetchMeta, TransferError> {
    let file: File = client.get(&format!("/api/v1/files/{file_id}")).await?;
    if file.id != file_id {
        return Err(ApiError::Decode.into());
    }
    let now = jiff::Timestamp::now();
    let locked = file.locked_for_user.or(file.locked) == Some(true)
        || file.unlock_at.as_value().is_some_and(|at| *at > now)
        || file.lock_at.as_value().is_some_and(|at| *at <= now);
    // Zero is a valid size. Unknown size must never borrow stale listing data.
    let size = match file.size {
        Some(size) => size,
        None if locked => 0,
        None => return Err(ApiError::Decode.into()),
    };
    Ok(FileFetchMeta {
        remote: RemoteMeta {
            size,
            updated_at: file.updated_at.map(|t| t.to_string()),
        },
        locked,
        file_id,
        url: file.url,
    })
}

/// Fresh metadata and one `StorageExpired` refresh; never resume partial bodies.
#[derive(Clone)]
pub struct ApiTransfer {
    client: Client,
    progress: Option<ProgressFn>,
    metadata: Option<FileFetchMeta>,
}

impl ApiTransfer {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            progress: None,
            metadata: None,
        }
    }

    /// Reuse the exact metadata used for the install's clobber decision.
    #[must_use]
    pub fn with_metadata(mut self, metadata: FileFetchMeta) -> Self {
        self.metadata = Some(metadata);
        self
    }

    #[must_use]
    pub fn with_progress(mut self, progress: ProgressFn) -> Self {
        self.progress = Some(progress);
        self
    }
}

impl Transfer for ApiTransfer {
    async fn fetch(
        &self,
        file_id: i64,
        sink: &mut (dyn AsyncWrite + Send + Unpin),
        expected_size: u64,
    ) -> Result<u64, TransferError> {
        let initial = match &self.metadata {
            Some(meta) if meta.file_id == file_id => meta.clone(),
            _ => file_remote_meta(&self.client, file_id).await?,
        };
        let mut current = initial.clone();
        for attempt in 0..2 {
            // A refreshed URL does not grant permission to ignore a new lock or
            // to install a different revision under the initial manifest metadata.
            if current.locked {
                return Err(TransferError::Message("locked".into()));
            }
            if current.remote.size != expected_size
                || current.remote.updated_at != initial.remote.updated_at
            {
                return Err(TransferError::Message(
                    "remote file changed during transfer; rerun".into(),
                ));
            }
            let url = current
                .url
                .ok_or_else(|| TransferError::Message("missing download URL".into()))?;
            let progress = self.progress.as_ref();
            let result = api_download(&self.client, url, &mut *sink, Some(expected_size), |n| {
                if let Some(cb) = progress {
                    cb(file_id, n);
                }
            })
            .await;
            match result {
                Ok(n) => return Ok(n),
                // Status is checked before writing bytes, so this retry reuses
                // an empty sink. Other incomplete transfers fail and clean up.
                Err(ApiError::StorageExpired) if attempt == 0 => {
                    current = file_remote_meta(&self.client, file_id).await?;
                }
                Err(ApiError::StorageExpired) => {
                    return Err(TransferError::Message(
                        "storage URL expired after refresh".into(),
                    ));
                }
                Err(ApiError::SizeMismatch) => return Err(TransferError::SizeMismatch),
                Err(e) => return Err(e.into()),
            }
        }
        unreachable!("two attempts always return")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

    #[tokio::test]
    async fn fresh_metadata_distinguishes_empty_unknown_and_effective_access() {
        let server = MockServer::start().await;
        let client = Client::new(
            server.uri().parse().unwrap(),
            canvas_api::Secret::new("tok"),
            "test",
        )
        .unwrap();
        for (body, expected) in [
            (json!({"id": 50, "size": 0}), Some((0, false))),
            (json!({"id": 50}), None),
            (json!({"id": 51, "size": 4}), None),
            (json!({"id": 50, "locked_for_user": true}), Some((0, true))),
            (
                json!({"id": 50, "size": 4, "locked": true, "locked_for_user": false}),
                Some((4, false)),
            ),
        ] {
            server.reset().await;
            Mock::given(path("/api/v1/files/50"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            let result = file_remote_meta(&client, 50).await;
            match expected {
                Some((size, locked)) => {
                    let result = result.unwrap();
                    assert_eq!(result.remote.size, size);
                    assert_eq!(result.locked, locked);
                }
                None => assert!(matches!(result, Err(TransferError::Api(ApiError::Decode)))),
            }
        }
    }

    #[tokio::test]
    async fn metadata_errors_keep_the_api_variant() {
        let server = MockServer::start().await;
        let client = Client::new(
            server.uri().parse().unwrap(),
            canvas_api::Secret::new("tok"),
            "test",
        )
        .unwrap();
        Mock::given(path("/api/v1/files/50"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        assert!(matches!(
            file_remote_meta(&client, 50).await,
            Err(TransferError::Api(ApiError::Unauthorized))
        ));
    }
}
