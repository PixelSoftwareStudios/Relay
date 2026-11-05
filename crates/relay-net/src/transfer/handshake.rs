//! Noise XX handshake of a data link.
//!
//! Same pattern, preface and key checks as the control handshake, with a different prologue
//! domain (so a control link can never be completed as a data link) and one addition: the
//! initiator's third message carries the transfer ticket as its Noise payload, encrypted under
//! keys that already include both static keys. The initiator pins the responder's expected key
//! before revealing its own key or the token; the responder checks the ticket before it
//! confirms anything, and answers a bad ticket with a plain close.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;

use super::DataError;
use super::token::{TOKEN_BYTES, TransferToken};
use crate::handshake::framing::{read_frame, write_frame};
use crate::handshake::{
	HandshakeError, decode_preface, encode_preface, finish, noise_state, remote_key,
};
use crate::identity::{Fingerprint, Identity};
use crate::limits::{
	HANDSHAKE_MESSAGE_MAX_BYTES, HANDSHAKE_TIMEOUT, PREFACE_BYTES, PREFACE_TIMEOUT,
};
use crate::noise::Purpose;
use crate::session::{LinkEpoch, Role, SecureSession};

const TICKET_MAGIC: &[u8; 4] = b"RDT1";
const TICKET_BYTES: usize = 4 + 16 + 16 + TOKEN_BYTES;

/// What the dialer presents.
#[derive(Debug, Clone)]
pub struct Ticket {
	pub session_id: [u8; 16],
	pub offer_id: [u8; 16],
	pub token: TransferToken,
}

impl Ticket {
	fn encode(&self) -> [u8; TICKET_BYTES] {
		let mut out = [0u8; TICKET_BYTES];
		out[..4].copy_from_slice(TICKET_MAGIC);
		out[4..20].copy_from_slice(&self.session_id);
		out[20..36].copy_from_slice(&self.offer_id);
		out[36..].copy_from_slice(self.token.as_bytes());
		out
	}

	fn decode(raw: &[u8]) -> Option<Self> {
		if raw.len() != TICKET_BYTES || &raw[..4] != TICKET_MAGIC {
			return None;
		}
		Some(Self {
			session_id: raw[4..20].try_into().ok()?,
			offer_id: raw[20..36].try_into().ok()?,
			token: TransferToken::from_slice(&raw[36..])?,
		})
	}
}

fn noise<E>(_: E) -> DataError {
	DataError::Handshake(HandshakeError::Noise)
}

/// Dialer side. `expected` is checked after message 2, before our key or the token is sent.
pub async fn initiate<S: AsyncRead + AsyncWrite + Unpin>(
	stream: &mut S,
	identity: &Identity,
	expected: &Fingerprint,
	ticket: &Ticket,
) -> Result<SecureSession, DataError> {
	timeout(HANDSHAKE_TIMEOUT, async {
		let epoch = LinkEpoch::random();
		stream
			.write_all(&encode_preface(&epoch))
			.await
			.map_err(HandshakeError::Io)?;
		let mut state = noise_state(Purpose::Data, identity, &epoch, Role::Initiator)?;
		let mut buf = [0u8; HANDSHAKE_MESSAGE_MAX_BYTES];
		let n = state.write_message(&[], &mut buf).map_err(noise)?;
		write_frame(stream, &buf[..n]).await?;
		let frame = read_frame(stream).await?;
		let mut scratch = [0u8; HANDSHAKE_MESSAGE_MAX_BYTES];
		if state.read_message(&frame, &mut scratch).map_err(noise)? != 0 {
			return Err(noise(()));
		}
		if &remote_key(&state, identity)?.fingerprint() != expected {
			return Err(DataError::Handshake(HandshakeError::UnexpectedPeer));
		}
		let n = state
			.write_message(&ticket.encode(), &mut buf)
			.map_err(noise)?;
		write_frame(stream, &buf[..n]).await?;
		Ok(finish(state, identity, epoch, Role::Initiator)?)
	})
	.await
	.map_err(|_| DataError::Handshake(HandshakeError::Timeout))?
}

/// Listener side. `admit` sees the authenticated peer fingerprint and the presented ticket and
/// decides; its value is returned with the session. Any refusal is `TokenRejected`.
pub async fn respond<S, T, F>(
	stream: &mut S,
	identity: &Identity,
	admit: F,
) -> Result<(SecureSession, T), DataError>
where
	S: AsyncRead + AsyncWrite + Unpin,
	F: FnOnce(&Fingerprint, &Ticket) -> Result<T, DataError>,
{
	timeout(HANDSHAKE_TIMEOUT, async {
		let mut raw = [0u8; PREFACE_BYTES];
		timeout(PREFACE_TIMEOUT, stream.read_exact(&mut raw))
			.await
			.map_err(|_| HandshakeError::Timeout)?
			.map_err(HandshakeError::Io)?;
		let epoch = decode_preface(&raw)?;
		let mut state = noise_state(Purpose::Data, identity, &epoch, Role::Responder)?;
		let mut scratch = [0u8; HANDSHAKE_MESSAGE_MAX_BYTES];
		let frame = read_frame(stream).await?;
		if state.read_message(&frame, &mut scratch).map_err(noise)? != 0 {
			return Err(noise(()));
		}
		let mut buf = [0u8; HANDSHAKE_MESSAGE_MAX_BYTES];
		let n = state.write_message(&[], &mut buf).map_err(noise)?;
		write_frame(stream, &buf[..n]).await?;
		let frame = read_frame(stream).await?;
		let n = state.read_message(&frame, &mut scratch).map_err(noise)?;
		let peer = remote_key(&state, identity)?.fingerprint();
		let ticket = Ticket::decode(&scratch[..n]).ok_or(DataError::TokenRejected)?;
		let admitted = admit(&peer, &ticket)?;
		Ok((finish(state, identity, epoch, Role::Responder)?, admitted))
	})
	.await
	.map_err(|_| DataError::Handshake(HandshakeError::Timeout))?
}
