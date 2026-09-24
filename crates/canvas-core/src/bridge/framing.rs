//! Chrome native-messaging framing.
//!
//! A message is a 4-byte length prefix in **native** byte order followed by
//! that many bytes of UTF-8 JSON. Chrome's own limit is larger than anything
//! this companion sends, so the bound here is the companion's: 1 MiB, and it
//! is checked against the header **before** a buffer is allocated. A host that
//! allocated first would let a hostile four-byte header ask for four
//! gigabytes.

use std::io::{self, Read, Write};

use thiserror::Error;

/// The largest message this bridge reads or writes, in bytes.
///
/// The bounded browser payload is 64 KiB; the rest is envelope
/// and headroom. Chrome permits more, so this is the tighter of the two.
pub const MAX_MESSAGE_BYTES: u32 = 1024 * 1024;

/// A framing failure. None of these leave the stream usable.
#[derive(Debug, Error)]
pub enum FramingError {
    /// The header asked for more than [`MAX_MESSAGE_BYTES`]. Nothing was
    /// allocated and nothing past the header was read.
    #[error("message of {len} bytes exceeds the {MAX_MESSAGE_BYTES} byte limit")]
    Oversize { len: u32 },
    /// The stream ended in the middle of a header or a body.
    #[error("truncated native message")]
    Truncated,
    /// The body was not UTF-8, or not the JSON this protocol defines.
    #[error("malformed native message: {0}")]
    Malformed(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Read one framed message, or `None` at a clean end of stream.
///
/// The length is validated against [`MAX_MESSAGE_BYTES`] before the body
/// buffer exists, so an oversize header costs four bytes and no memory.
pub fn read_message(reader: &mut impl Read) -> Result<Option<Vec<u8>>, FramingError> {
    let mut header = [0u8; 4];
    if !read_exact_or_eof(reader, &mut header)? {
        // A clean end of stream: Chrome closed the pipe.
        return Ok(None);
    }
    let len = u32::from_ne_bytes(header);
    if len > MAX_MESSAGE_BYTES {
        return Err(FramingError::Oversize { len });
    }
    // `len` is bounded above, so this allocation is bounded too.
    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => FramingError::Truncated,
        _ => FramingError::Io(e),
    })?;
    Ok(Some(body))
}

/// Write one framed message. Refuses to send more than the limit permits.
pub fn write_message(writer: &mut impl Write, body: &[u8]) -> Result<(), FramingError> {
    let len = u32::try_from(body.len()).unwrap_or(u32::MAX);
    if len > MAX_MESSAGE_BYTES {
        return Err(FramingError::Oversize { len });
    }
    writer.write_all(&len.to_ne_bytes())?;
    writer.write_all(body)?;
    writer.flush()?;
    Ok(())
}

/// Read and deserialize one framed JSON message.
pub fn read_json<T: serde::de::DeserializeOwned>(
    reader: &mut impl Read,
) -> Result<Option<T>, FramingError> {
    let Some(body) = read_message(reader)? else {
        return Ok(None);
    };
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| FramingError::Malformed(e.to_string()))
}

/// Serialize and write one framed JSON message.
pub fn write_json<T: serde::Serialize>(
    writer: &mut impl Write,
    value: &T,
) -> Result<(), FramingError> {
    let body = serde_json::to_vec(value).map_err(|e| FramingError::Malformed(e.to_string()))?;
    write_message(writer, &body)
}

/// Fill `buf`, reporting a clean end of stream as `Ok(false)`.
fn read_exact_or_eof(reader: &mut impl Read, buf: &mut [u8]) -> Result<bool, FramingError> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(FramingError::Truncated),
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(FramingError::Io(e)),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that records how many bytes were actually taken from it.
    struct Counting<'a> {
        bytes: &'a [u8],
        read: usize,
    }

    impl Read for Counting<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.bytes.len() - self.read);
            buf[..n].copy_from_slice(&self.bytes[self.read..self.read + n]);
            self.read += n;
            Ok(n)
        }
    }

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(body.len()).unwrap().to_ne_bytes().to_vec();
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn a_round_trip_preserves_the_body() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, b"{\"type\":\"hello\"}").unwrap();
        let mut cursor = buffer.as_slice();
        assert_eq!(
            read_message(&mut cursor).unwrap().as_deref(),
            Some(&b"{\"type\":\"hello\"}"[..])
        );
        assert_eq!(read_message(&mut cursor).unwrap(), None);
    }

    /// M7-a acceptance: an oversize length is rejected **before** allocation.
    ///
    /// Nothing but the four header bytes is taken from the stream, so no
    /// buffer of the declared size was ever created.
    #[test]
    fn an_oversize_length_is_refused_before_the_body_is_allocated() {
        let mut bytes = (MAX_MESSAGE_BYTES + 1).to_ne_bytes().to_vec();
        bytes.extend_from_slice(b"this body is never read");
        let mut reader = Counting {
            bytes: &bytes,
            read: 0,
        };
        let error = read_message(&mut reader).unwrap_err();
        assert!(matches!(error, FramingError::Oversize { len } if len == MAX_MESSAGE_BYTES + 1));
        assert_eq!(reader.read, 4, "only the header may be consumed");
    }

    /// The same bound holds for the extreme header a hostile peer would send.
    #[test]
    fn a_four_gigabyte_header_allocates_nothing() {
        let bytes = u32::MAX.to_ne_bytes().to_vec();
        let mut reader = Counting {
            bytes: &bytes,
            read: 0,
        };
        assert!(matches!(
            read_message(&mut reader).unwrap_err(),
            FramingError::Oversize { .. }
        ));
        assert_eq!(reader.read, 4);
    }

    #[test]
    fn the_limit_itself_is_accepted() {
        let body = vec![b'x'; MAX_MESSAGE_BYTES as usize];
        let bytes = framed(&body);
        let mut cursor = bytes.as_slice();
        assert_eq!(
            read_message(&mut cursor).unwrap().unwrap().len(),
            body.len()
        );
    }

    #[test]
    fn a_writer_refuses_to_send_more_than_the_limit() {
        let body = vec![b'x'; MAX_MESSAGE_BYTES as usize + 1];
        let mut out = Vec::new();
        assert!(matches!(
            write_message(&mut out, &body).unwrap_err(),
            FramingError::Oversize { .. }
        ));
        assert!(out.is_empty(), "nothing may reach the wire");
    }

    #[test]
    fn a_truncated_body_is_not_a_message() {
        let mut bytes = 16u32.to_ne_bytes().to_vec();
        bytes.extend_from_slice(b"only four");
        let mut cursor = bytes.as_slice();
        assert!(matches!(
            read_message(&mut cursor).unwrap_err(),
            FramingError::Truncated
        ));
    }

    #[test]
    fn a_truncated_header_is_not_a_message() {
        let mut cursor = &b"\x01\x02"[..];
        assert!(matches!(
            read_message(&mut cursor).unwrap_err(),
            FramingError::Truncated
        ));
    }

    #[test]
    fn a_body_that_is_not_this_protocol_is_malformed() {
        let bytes = framed(b"not json");
        let mut cursor = bytes.as_slice();
        assert!(matches!(
            read_json::<serde_json::Value>(&mut cursor).unwrap_err(),
            FramingError::Malformed(_)
        ));
    }
}
