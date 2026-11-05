use std::sync::Arc;

use prost::Message;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::{ControlError, validate_envelope};
use crate::limits::CONTROL_RECORD_MAX_BYTES;
use crate::proto::Envelope;
use crate::record::RecordKind;
use crate::session::SecureSession;

pub struct ControlSender<W> {
	writer: W,
	session: Arc<SecureSession>,
	failed: bool,
}

impl<W: AsyncWrite + Unpin> ControlSender<W> {
	pub(super) fn new(writer: W, session: Arc<SecureSession>) -> Self {
		Self {
			writer,
			session,
			failed: false,
		}
	}

	/// Validates, seals and writes one envelope. A failure may have consumed a nonce and is
	/// terminal for this half.
	pub async fn send(&mut self, envelope: &Envelope) -> Result<(), ControlError> {
		if self.failed {
			return Err(ControlError::Closed);
		}
		let result = self.send_inner(envelope).await;
		self.failed = result.is_err();
		result
	}

	async fn send_inner(&mut self, envelope: &Envelope) -> Result<(), ControlError> {
		validate_envelope(envelope)?;
		let record = self
			.session
			.seal(RecordKind::Envelope, &envelope.encode_to_vec())?;
		if record.len() > CONTROL_RECORD_MAX_BYTES {
			return Err(ControlError::BadLength);
		}
		let mut frame = Vec::with_capacity(4 + record.len());
		frame.extend_from_slice(&(record.len() as u32).to_be_bytes());
		frame.extend_from_slice(&record);
		self.writer.write_all(&frame).await?;
		self.writer.flush().await?;
		Ok(())
	}

	pub async fn shutdown(&mut self) -> Result<(), ControlError> {
		self.writer.shutdown().await?;
		Ok(())
	}
}
