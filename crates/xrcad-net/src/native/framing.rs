//! Length-prefixed frame codec for the xrcad TCP transport.
//!
//! Wire format: `[u32 LE length][payload bytes]`

use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAX_FRAME: usize = 4 * 1024 * 1024; // 4 MiB

/// Write a single frame: 4-byte LE length prefix followed by `data`.
pub async fn write_frame<W: AsyncWriteExt + Unpin>(
    w: &mut W,
    data: &[u8],
) -> std::io::Result<()> {
    w.write_all(&(data.len() as u32).to_le_bytes()).await?;
    w.write_all(data).await
}

/// Read a single frame: parse the 4-byte LE length then read that many bytes.
pub async fn read_frame<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Build a pre-assembled frame (length prefix + payload) without I/O.
///
/// Used by the coordinator to create frames that the per-peer writer tasks
/// can forward directly to the TCP socket.
pub fn make_frame(data: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(4 + data.len());
    frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
    frame.extend_from_slice(data);
    frame
}
