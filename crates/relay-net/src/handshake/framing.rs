use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::HandshakeError;
use crate::limits::HANDSHAKE_MESSAGE_MAX_BYTES;

/// Reads a 4-byte big-endian length then that many bytes. The length is validated before
/// any buffer is allocated.
pub(super) async fn read_frame<S: AsyncRead + Unpin>(
	stream: &mut S,
) -> Result<Vec<u8>, HandshakeError> {
	let mut len = [0u8; 4];
	stream.read_exact(&mut len).await?;
	let len = u32::from_be_bytes(len) as usize;
	if len == 0 || len > HANDSHAKE_MESSAGE_MAX_BYTES {
		return Err(HandshakeError::BadMessageLength);
	}
	let mut frame = vec![0u8; len];
	stream.read_exact(&mut frame).await?;
	Ok(frame)
}

pub(super) async fn write_frame<S: AsyncWrite + Unpin>(
	stream: &mut S,
	frame: &[u8],
) -> Result<(), HandshakeError> {
	let len = u32::try_from(frame.len()).map_err(|_| HandshakeError::BadMessageLength)?;
	stream.write_all(&len.to_be_bytes()).await?;
	stream.write_all(frame).await?;
	stream.flush().await?;
	Ok(())
}
