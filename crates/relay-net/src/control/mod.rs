//! Reliable peer-control channel: length-prefixed, size-bounded, Noise-protected records
//! carrying `relay.peer.v1` envelopes over any ordered byte stream (TCP in production).
//!
//! Every error here is terminal for the channel. TCP is ordered, so a bad record means the
//! peer is faulty or hostile, and nothing is mutated before a record is fully accepted.

mod receiver;
mod sender;
mod validate;

use std::sync::Arc;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite, ReadHalf, WriteHalf, split};

pub use receiver::ControlReceiver;
pub use sender::ControlSender;
pub use validate::{ValidationError, new_envelope, new_hello, new_message_id, validate_envelope};

use crate::session::{OpenError, SealError, SecureSession};

#[derive(Debug, Error)]
pub enum ControlError {
	#[error("i/o error: {0}")]
	Io(#[from] std::io::Error),
	#[error("record length out of bounds")]
	BadLength,
	#[error("record rejected: {0}")]
	Record(#[from] OpenError),
	#[error("record is not a control envelope on the control lane")]
	WrongKind,
	#[error("control sequence is not strictly increasing")]
	Replay,
	#[error("envelope failed to decode")]
	Decode,
	#[error("envelope invalid: {0}")]
	Invalid(#[from] ValidationError),
	#[error("first message must be a Hello bound to the authenticated key")]
	HelloRequired,
	#[error("duplicate message id with a different body")]
	ConflictingDuplicate,
	#[error("failed to protect outgoing record: {0}")]
	Seal(#[from] SealError),
	#[error("channel closed after an earlier error")]
	Closed,
}

/// Splits a handshaken stream into independent send and receive halves.
pub fn control_channel<S: AsyncRead + AsyncWrite>(
	stream: S,
	session: Arc<SecureSession>,
) -> (ControlSender<WriteHalf<S>>, ControlReceiver<ReadHalf<S>>) {
	let (read, write) = split(stream);
	(
		ControlSender::new(write, Arc::clone(&session)),
		ControlReceiver::new(read, session),
	)
}
