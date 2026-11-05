//! Encrypted UDP media datagrams carrying opaque payloads (RTP is built elsewhere).

mod datagram;
mod endpoint;
mod path;
mod receiver;
mod replay;

pub use datagram::{BodyError, CHALLENGE_BYTES, CallId, MAX_PAYLOAD_BYTES};
pub use endpoint::{AudioPacket, MediaEndpoint, MediaError};
pub use path::{PathValidator, ResponseLimiter};
pub use receiver::{MediaReceiver, Received, RejectReason};
pub use replay::{ReplayVerdict, ReplayWindow};

use crate::record::RecordKind;
use crate::session::{SealError, SecureSession};

/// Seals one audio datagram. The nonce comes from the session's media-lane counter, never
/// from anything inside the payload.
pub fn seal_audio(
	session: &SecureSession,
	call_id: &CallId,
	generation: u32,
	payload: &[u8],
) -> Result<Vec<u8>, MediaError> {
	let body = datagram::encode_audio(call_id, generation, payload).map_err(MediaError::Body)?;
	Ok(session.seal(RecordKind::Audio, &body)?)
}

pub(crate) fn seal_probe(
	session: &SecureSession,
	kind: RecordKind,
	value: &[u8; CHALLENGE_BYTES],
) -> Result<Vec<u8>, SealError> {
	session.seal(kind, value)
}
