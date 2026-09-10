//! Blocking I/O bridge onto the async runtime (§13).

use std::io::{Read, Write};

use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio::task::spawn_blocking;

/// Chunk size for body bridges (64 KiB).
pub const CHUNK_SIZE: usize = 64 * 1024;

/// Channel capacity in chunks (backpressure bound).
pub const CHANNEL_CAPACITY: usize = 8;

/// Error from the blocking bridge.
#[derive(Debug, thiserror::Error)]
pub enum IoBridgeError {
    /// Channel closed early.
    #[error("bridge channel closed")]
    Closed,
    /// Blocking task failed to join.
    #[error("blocking task join error")]
    Join,
    /// I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Run `f` on the blocking pool.
pub async fn run_blocking<F, T>(f: F) -> Result<T, IoBridgeError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    spawn_blocking(f).await.map_err(|_| IoBridgeError::Join)
}

/// `fsync` a std file on a blocking worker.
pub async fn fsync_std(file: std::fs::File) -> Result<std::fs::File, IoBridgeError> {
    Ok(run_blocking(move || {
        file.sync_all()?;
        Ok::<_, std::io::Error>(file)
    })
    .await??)
}

/// Hash bytes on a blocking worker.
pub async fn hash_sha256(data: Vec<u8>) -> Result<[u8; 32], IoBridgeError> {
    run_blocking(move || {
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let out = hasher.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&out);
        arr
    })
    .await
}

/// Hash a readable source in 64 KiB chunks on the blocking pool.
pub async fn hash_reader_sha256<R>(mut reader: R) -> Result<[u8; 32], IoBridgeError>
where
    R: Read + Send + 'static,
{
    Ok(run_blocking(move || {
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; CHUNK_SIZE];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let out = hasher.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&out);
        Ok::<_, std::io::Error>(arr)
    })
    .await??)
}

/// Async → blocking writer bridge with 64 KiB chunks and backpressure.
///
/// Returns a sender; drop the sender to close. The join handle finishes after EOF.
pub fn bridge_to_blocking_writer<W>(
    mut writer: W,
) -> (
    mpsc::Sender<Vec<u8>>,
    tokio::task::JoinHandle<Result<u64, std::io::Error>>,
)
where
    W: Write + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAPACITY);
    let handle = spawn_blocking(move || {
        let mut total = 0u64;
        while let Some(chunk) = rx.blocking_recv() {
            writer.write_all(&chunk)?;
            total += chunk.len() as u64;
        }
        writer.flush()?;
        Ok(total)
    });
    (tx, handle)
}

/// Blocking reader → async receiver bridge with 64 KiB chunks and backpressure.
pub fn bridge_from_blocking_reader<R>(
    mut reader: R,
) -> (
    mpsc::Receiver<Vec<u8>>,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
)
where
    R: Read + Send + 'static,
{
    let (tx, rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAPACITY);
    let handle = spawn_blocking(move || {
        let mut buf = vec![0u8; CHUNK_SIZE];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            if tx.blocking_send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
        Ok(())
    });
    (rx, handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Buf(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn round_trip_bridge_and_hash() {
        let data = vec![42u8; CHUNK_SIZE + 100];
        let expected = {
            let mut h = Sha256::new();
            h.update(&data);
            h.finalize()
        };

        let store = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (tx, handle) = bridge_to_blocking_writer(Buf(store.clone()));
        tx.send(data.clone()).await.unwrap();
        drop(tx);
        let written = handle.await.unwrap().unwrap();
        assert_eq!(written, data.len() as u64);
        let got = store.lock().unwrap().clone();
        assert_eq!(got, data);

        let hash = hash_sha256(data).await.unwrap();
        assert_eq!(&hash[..], &expected[..]);
    }
}
