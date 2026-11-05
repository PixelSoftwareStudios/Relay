//! TCP preface plus Noise XX handshake. Completing it proves key possession only; trust and
//! call acceptance are separate decisions made by the caller afterwards.

mod framing;
mod gate;

use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::timeout;

pub use gate::{HandshakeGate, HandshakePermit};

use crate::identity::{Fingerprint, Identity, KEY_BYTES, PublicKey};
use crate::limits::{EPOCH_BYTES, HANDSHAKE_MESSAGE_MAX_BYTES, HANDSHAKE_TIMEOUT, PREFACE_BYTES};
use crate::noise::{PROTOCOL_MAJOR, builder, prologue};
use crate::session::{LinkEpoch, Role, SecureSession};

const PREFACE_MAGIC: &[u8; 4] = b"RLH1";

#[derive(Debug, Error)]
pub enum HandshakeError {
	#[error("i/o error during handshake: {0}")]
	Io(#[from] std::io::Error),
	#[error("handshake timed out")]
	Timeout,
	#[error("invalid preface")]
	BadPreface,
	#[error("unsupported protocol major {0}")]
	UnsupportedMajor(u16),
	#[error("handshake message has an invalid length")]
	BadMessageLength,
	#[error("noise handshake failed")]
	Noise,
	#[error("remote key does not match the expected fingerprint")]
	UnexpectedPeer,
	#[error("too many pending handshakes")]
	Overloaded,
}

fn encode_preface(epoch: &LinkEpoch) -> [u8; PREFACE_BYTES] {
	let mut out = [0u8; PREFACE_BYTES];
	out[..4].copy_from_slice(PREFACE_MAGIC);
	out[4..6].copy_from_slice(&PROTOCOL_MAJOR.to_be_bytes());
	// bytes 6..8 reserved flags stay zero
	out[8..8 + EPOCH_BYTES].copy_from_slice(epoch.as_bytes());
	out
}

fn decode_preface(raw: &[u8; PREFACE_BYTES]) -> Result<LinkEpoch, HandshakeError> {
	if &raw[..4] != PREFACE_MAGIC || raw[6] != 0 || raw[7] != 0 {
		return Err(HandshakeError::BadPreface);
	}
	let major = u16::from_be_bytes([raw[4], raw[5]]);
	if major != PROTOCOL_MAJOR {
		return Err(HandshakeError::UnsupportedMajor(major));
	}
	LinkEpoch::from_slice(&raw[8..]).ok_or(HandshakeError::BadPreface)
}

fn noise_state(
	identity: &Identity,
	epoch: &LinkEpoch,
	role: Role,
) -> Result<snow::HandshakeState, HandshakeError> {
	let prologue = prologue(PROTOCOL_MAJOR, epoch);
	let builder = builder()
		.local_private_key(identity.secret_bytes())
		.and_then(|b| b.prologue(&prologue))
		.map_err(|_| HandshakeError::Noise)?;
	match role {
		Role::Initiator => builder.build_initiator(),
		Role::Responder => builder.build_responder(),
	}
	.map_err(|_| HandshakeError::Noise)
}

fn remote_key(state: &snow::HandshakeState) -> Result<PublicKey, HandshakeError> {
	let raw = state.get_remote_static().ok_or(HandshakeError::Noise)?;
	let bytes: [u8; KEY_BYTES] = raw.try_into().map_err(|_| HandshakeError::Noise)?;
	Ok(PublicKey::from_bytes(bytes))
}

/// Writes the next handshake message and sends it.
async fn send_step<S: AsyncWrite + Unpin>(
	stream: &mut S,
	state: &mut snow::HandshakeState,
) -> Result<(), HandshakeError> {
	let mut buf = [0u8; HANDSHAKE_MESSAGE_MAX_BYTES];
	let n = state
		.write_message(&[], &mut buf)
		.map_err(|_| HandshakeError::Noise)?;
	framing::write_frame(stream, &buf[..n]).await
}

/// Receives and processes the next handshake message. Payloads must be empty.
async fn recv_step<S: AsyncRead + Unpin>(
	stream: &mut S,
	state: &mut snow::HandshakeState,
) -> Result<(), HandshakeError> {
	let frame = framing::read_frame(stream).await?;
	let mut payload = [0u8; HANDSHAKE_MESSAGE_MAX_BYTES];
	let n = state
		.read_message(&frame, &mut payload)
		.map_err(|_| HandshakeError::Noise)?;
	if n != 0 {
		return Err(HandshakeError::Noise);
	}
	Ok(())
}

fn finish(
	state: snow::HandshakeState,
	epoch: LinkEpoch,
	role: Role,
) -> Result<SecureSession, HandshakeError> {
	if !state.is_handshake_finished() {
		return Err(HandshakeError::Noise);
	}
	let remote = remote_key(&state)?;
	let transport = state
		.into_stateless_transport_mode()
		.map_err(|_| HandshakeError::Noise)?;
	Ok(SecureSession::new(transport, epoch, role, remote))
}

/// Runs the initiator side. When `expected` is given, the responder's key is checked after
/// message 2 and before our static key is revealed in message 3.
pub async fn initiate<S: AsyncRead + AsyncWrite + Unpin>(
	stream: &mut S,
	identity: &Identity,
	expected: Option<&Fingerprint>,
) -> Result<SecureSession, HandshakeError> {
	timeout(
		HANDSHAKE_TIMEOUT,
		initiate_inner(stream, identity, expected),
	)
	.await
	.map_err(|_| HandshakeError::Timeout)?
}

async fn initiate_inner<S: AsyncRead + AsyncWrite + Unpin>(
	stream: &mut S,
	identity: &Identity,
	expected: Option<&Fingerprint>,
) -> Result<SecureSession, HandshakeError> {
	use tokio::io::AsyncWriteExt;

	let epoch = LinkEpoch::random();
	stream.write_all(&encode_preface(&epoch)).await?;
	let mut state = noise_state(identity, &epoch, Role::Initiator)?;
	send_step(stream, &mut state).await?;
	recv_step(stream, &mut state).await?;
	if let Some(expected) = expected
		&& &remote_key(&state)?.fingerprint() != expected
	{
		return Err(HandshakeError::UnexpectedPeer);
	}
	send_step(stream, &mut state).await?;
	finish(state, epoch, Role::Initiator)
}

/// Runs the responder side.
pub async fn respond<S: AsyncRead + AsyncWrite + Unpin>(
	stream: &mut S,
	identity: &Identity,
) -> Result<SecureSession, HandshakeError> {
	timeout(HANDSHAKE_TIMEOUT, respond_inner(stream, identity))
		.await
		.map_err(|_| HandshakeError::Timeout)?
}

async fn respond_inner<S: AsyncRead + AsyncWrite + Unpin>(
	stream: &mut S,
	identity: &Identity,
) -> Result<SecureSession, HandshakeError> {
	use tokio::io::AsyncReadExt;

	let mut raw = [0u8; PREFACE_BYTES];
	stream.read_exact(&mut raw).await?;
	let epoch = decode_preface(&raw)?;
	let mut state = noise_state(identity, &epoch, Role::Responder)?;
	recv_step(stream, &mut state).await?;
	send_step(stream, &mut state).await?;
	recv_step(stream, &mut state).await?;
	finish(state, epoch, Role::Responder)
}

/// Handshake timeout, exposed so callers can size their own surrounding deadlines.
pub const fn handshake_timeout() -> Duration {
	HANDSHAKE_TIMEOUT
}
