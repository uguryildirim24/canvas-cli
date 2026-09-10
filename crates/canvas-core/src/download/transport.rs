//! Wire `canvas-api::download` into the install [`Transfer`] trait.

use std::sync::Arc;

use tokio::io::AsyncWrite;

use canvas_api::download::download as api_download;
use canvas_api::models::File;
use canvas_api::{Client, Error as ApiError};

use crate::download::install::{RemoteMeta, Transfer, TransferError};

/// Progress callback: `(file_id, cumulative_bytes)`.
pub type ProgressFn = Arc<dyn Fn(i64, u64) + Send + Sync>;

/// Fresh file metadata for install planning.
#[derive(Debug, Clone)]
pub struct FileFetchMeta {
    /// Remote size / `updated_at` for the clobber table.
    pub remote: RemoteMeta,
    /// True when Canvas reports the file locked for the user.
    pub locked: bool,
}

/// `GET /api/v1/files/:id` → remote meta (and lock bit).
pub async fn file_remote_meta(
    client: &Client,
    file_id: i64,
) -> Result<FileFetchMeta, TransferError> {
    let file = fetch_file_meta(client, file_id).await?;
    let locked =
        file.locked_for_user == Some(true) || file.locked == Some(true) || file.url.is_none();
    let size = file.size.unwrap_or(0);
    let updated_at = file.updated_at.map(|t| t.to_string());
    Ok(FileFetchMeta {
        remote: RemoteMeta { size, updated_at },
        locked,
    })
}

/// Canvas API transport: fresh `GET /files/:id`, then transfer with one
/// `StorageExpired` URL refresh. Incomplete bodies are restarted (never resumed).
#[derive(Clone)]
pub struct ApiTransfer {
    client: Client,
    progress: Option<ProgressFn>,
}

impl ApiTransfer {
    /// Build a transfer adapter for `client`.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            progress: None,
        }
    }

    /// Attach a per-file byte progress hook.
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
        let meta = fetch_file_meta(&self.client, file_id).await?;
        if meta.locked_for_user == Some(true) || meta.locked == Some(true) {
            return Err(TransferError::Message("locked".into()));
        }
        let Some(url) = meta.url.clone() else {
            return Err(TransferError::Message("locked".into()));
        };
        let expected = meta.size.unwrap_or(expected_size);
        let progress = self.progress.clone();
        match transfer_once(
            &self.client,
            url,
            sink,
            expected,
            file_id,
            progress.as_ref(),
        )
        .await
        {
            Ok(n) => Ok(n),
            Err(TransferError::Message(m)) if m == "storage_expired" => {
                // Restart: caller installs into a fresh part file; this sink has
                // no body bytes on StorageExpired (status checked before stream).
                let meta = fetch_file_meta(&self.client, file_id).await?;
                let Some(url) = meta.url else {
                    return Err(TransferError::Message("locked".into()));
                };
                let expected = meta.size.unwrap_or(expected_size);
                match transfer_once(
                    &self.client,
                    url,
                    sink,
                    expected,
                    file_id,
                    progress.as_ref(),
                )
                .await
                {
                    Ok(n) => Ok(n),
                    Err(TransferError::Message(m)) if m == "storage_expired" => {
                        Err(TransferError::Message("failed".into()))
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }
}

async fn fetch_file_meta(client: &Client, file_id: i64) -> Result<File, TransferError> {
    let path = format!("/api/v1/files/{file_id}");
    match client.get::<File>(&path).await {
        Ok(file) => Ok(file),
        Err(ApiError::Unauthorized) => Err(TransferError::Message("unauthorized".into())),
        Err(ApiError::Denied { status }) => {
            Err(TransferError::Message(format!("unavailable:{status}")))
        }
        Err(ApiError::NotFound) => Err(TransferError::Message("unavailable:404".into())),
        Err(ApiError::Forbidden { rate_limited, .. }) if !rate_limited => {
            Err(TransferError::Message("unavailable:403".into()))
        }
        Err(e) => Err(TransferError::Message(e.to_string())),
    }
}

async fn transfer_once(
    client: &Client,
    url: reqwest::Url,
    sink: &mut (dyn AsyncWrite + Send + Unpin),
    expected: u64,
    file_id: i64,
    progress: Option<&ProgressFn>,
) -> Result<u64, TransferError> {
    let mut on_progress = |n: u64| {
        if let Some(cb) = progress {
            cb(file_id, n);
        }
    };
    match api_download(client, url, sink, Some(expected), &mut on_progress).await {
        Ok(n) => Ok(n),
        Err(ApiError::StorageExpired) => Err(TransferError::Message("storage_expired".into())),
        Err(ApiError::SizeMismatch) => Err(TransferError::SizeMismatch),
        Err(ApiError::Denied { status }) => {
            Err(TransferError::Message(format!("unavailable:{status}")))
        }
        Err(ApiError::Unauthorized) => Err(TransferError::Message("unauthorized".into())),
        Err(e) => Err(TransferError::Message(e.to_string())),
    }
}
