//! Receiving side of a transfer.

use super::offer::required_free_space;
use super::sink::FileSink;
use super::types::*;
use crate::error::TransferError;
use crate::limits::{MAX_CHUNK_BYTES, TRANSFER_WINDOW_BYTES};
use crate::message::FileOffer;

/// Receiver state machine. Drives a [`FileSink`] and verifies BLAKE3 while streaming.
pub struct Receiver {
	name: crate::SafeFileName,
	size: u64,
	hash: [u8; 32],
	state: TransferState,
	hasher: blake3::Hasher,
	received: u64,
	granted: u64,
	started: bool,
	committed_as: Option<String>,
}

impl Receiver {
	/// New receiver for an offer, in `Offered`.
	pub fn new(offer: &FileOffer) -> Self {
		Self {
			name: offer.name.clone(),
			size: offer.size,
			hash: offer.hash,
			state: TransferState::Offered,
			hasher: blake3::Hasher::new(),
			received: 0,
			granted: 0,
			started: false,
			committed_as: None,
		}
	}

	/// Current state.
	pub fn state(&self) -> TransferState {
		self.state
	}

	/// Bytes received so far.
	pub fn received(&self) -> u64 {
		self.received
	}

	/// Final file name after a successful commit.
	pub fn committed_as(&self) -> Option<&str> {
		self.committed_as.as_deref()
	}

	/// Refuses the offer.
	pub fn reject(&mut self) -> Result<Vec<Effect>, TransferError> {
		if self.state != TransferState::Offered {
			return Err(TransferError::WrongState);
		}
		self.state = TransferState::Rejected;
		Ok(vec![Effect::Send(TransferMsg::Reject)])
	}

	/// Accepts: checks free space, creates the temporary file, grants the first window.
	/// The concurrency limit is enforced by [`super::Transfers`].
	pub fn accept(&mut self, sink: &mut dyn FileSink) -> Result<Vec<Effect>, TransferError> {
		if self.state != TransferState::Offered {
			return Err(TransferError::WrongState);
		}
		let needed = required_free_space(self.size);
		let available = sink.free_space().map_err(|_| TransferError::NoSpace {
			needed,
			available: 0,
		})?;
		if available < needed {
			return Err(TransferError::NoSpace { needed, available });
		}
		if sink.create_temp(&self.name, self.size).is_err() {
			return Err(TransferError::NoSpace { needed, available });
		}
		self.started = true;
		self.state = TransferState::Accepted;
		self.granted = self.size.min(TRANSFER_WINDOW_BYTES);
		let fx = vec![
			Effect::Send(TransferMsg::Accept),
			Effect::Send(TransferMsg::Credit(self.granted)),
		];
		if self.size == 0 {
			self.state = TransferState::Verifying;
		}
		Ok(fx)
	}

	/// Local cancel at any non-terminal state.
	pub fn cancel(&mut self, sink: &mut dyn FileSink) -> Vec<Effect> {
		if self.state.is_terminal() {
			return Vec::new();
		}
		self.discard(sink);
		self.state = TransferState::Cancelled(CancelBy::Local);
		vec![Effect::Send(TransferMsg::Cancel)]
	}

	fn discard(&mut self, sink: &mut dyn FileSink) {
		if self.started {
			sink.abort();
		}
	}

	fn fail(
		&mut self,
		sink: &mut dyn FileSink,
		why: FailureReason,
		verdict: Verdict,
	) -> Vec<Effect> {
		self.discard(sink);
		self.state = TransferState::Failed(why);
		vec![Effect::Send(TransferMsg::Result(verdict))]
	}

	/// Handles a message from the sender.
	pub fn on_message(&mut self, msg: TransferMsg, sink: &mut dyn FileSink) -> Vec<Effect> {
		if self.state.is_terminal() {
			return Vec::new();
		}
		match msg {
			TransferMsg::Cancel => {
				self.discard(sink);
				self.state = TransferState::Cancelled(CancelBy::Peer);
				Vec::new()
			}
			TransferMsg::Chunk { offset, data } => self.on_chunk(offset, &data, sink),
			TransferMsg::Done => self.on_done(sink),
			_ => self.fail(sink, FailureReason::Protocol, Verdict::Failed),
		}
	}

	fn on_chunk(&mut self, offset: u64, data: &[u8], sink: &mut dyn FileSink) -> Vec<Effect> {
		if !matches!(
			self.state,
			TransferState::Accepted | TransferState::Transferring
		) {
			return self.fail(sink, FailureReason::Protocol, Verdict::Failed);
		}
		let end = self.received.saturating_add(data.len() as u64);
		if offset != self.received
			|| data.len() > MAX_CHUNK_BYTES
			|| data.is_empty()
			|| end > self.granted
		{
			return self.fail(sink, FailureReason::Protocol, Verdict::Failed);
		}
		if sink.write(data).is_err() {
			return self.fail(sink, FailureReason::Sink, Verdict::Failed);
		}
		self.hasher.update(data);
		self.received = end;
		self.state = TransferState::Transferring;
		let mut fx = Vec::new();
		if self.received == self.size {
			self.state = TransferState::Verifying;
		} else if self.granted < self.size
			&& self.granted - self.received <= TRANSFER_WINDOW_BYTES / 2
		{
			self.granted = self.size.min(self.received + TRANSFER_WINDOW_BYTES);
			fx.push(Effect::Send(TransferMsg::Credit(self.granted)));
		}
		fx
	}

	fn on_done(&mut self, sink: &mut dyn FileSink) -> Vec<Effect> {
		if self.state != TransferState::Verifying {
			return self.fail(sink, FailureReason::SizeMismatch, Verdict::SizeMismatch);
		}
		if sink.sync().is_err() {
			return self.fail(sink, FailureReason::Sink, Verdict::Failed);
		}
		let digest: [u8; 32] = self.hasher.finalize().into();
		if digest != self.hash {
			return self.fail(sink, FailureReason::HashMismatch, Verdict::HashMismatch);
		}
		match sink.commit() {
			Ok(name) => {
				self.committed_as = Some(name);
				self.state = TransferState::Completed;
				vec![Effect::Send(TransferMsg::Result(Verdict::Ok))]
			}
			Err(_) => self.fail(sink, FailureReason::Sink, Verdict::Failed),
		}
	}
}
