//! Sending side of a transfer.

use super::types::*;
use crate::limits::MAX_CHUNK_BYTES;
use crate::message::FileOffer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Check {
	None,
	Start,
	Final,
}

/// Sender state machine. The engine reads the file and checks the source; no I/O here.
#[derive(Debug)]
pub struct Sender {
	size: u64,
	state: TransferState,
	granted: u64,
	sent: u64,
	reading: Option<(u64, usize)>,
	check: Check,
}

impl Sender {
	/// New sender for an offer we made.
	pub fn new(offer: &FileOffer) -> Self {
		Self {
			size: offer.size,
			state: TransferState::Offered,
			granted: 0,
			sent: 0,
			reading: None,
			check: Check::None,
		}
	}

	/// Current state.
	pub fn state(&self) -> TransferState {
		self.state
	}

	/// Bytes handed to the network.
	pub fn sent(&self) -> u64 {
		self.sent
	}

	fn fail(&mut self, why: FailureReason) -> Vec<Effect> {
		self.state = TransferState::Failed(why);
		self.reading = None;
		vec![Effect::Send(TransferMsg::Cancel)]
	}

	/// Local cancel.
	pub fn cancel(&mut self) -> Vec<Effect> {
		if self.state.is_terminal() {
			return Vec::new();
		}
		self.state = TransferState::Cancelled(CancelBy::Local);
		self.reading = None;
		vec![Effect::Send(TransferMsg::Cancel)]
	}

	/// Handles a message from the receiver.
	pub fn on_message(&mut self, msg: TransferMsg) -> Vec<Effect> {
		if self.state.is_terminal() {
			return Vec::new();
		}
		match (msg, self.state) {
			(TransferMsg::Accept, TransferState::Offered) => {
				self.state = TransferState::Accepted;
				self.check = Check::Start;
				vec![Effect::CheckSource]
			}
			(TransferMsg::Reject, TransferState::Offered) => {
				self.state = TransferState::Rejected;
				Vec::new()
			}
			(TransferMsg::Credit(n), s) if s.is_active() => {
				if n > self.size || n < self.granted {
					return self.fail(FailureReason::Protocol);
				}
				self.granted = n;
				self.pump()
			}
			(TransferMsg::Cancel, _) => {
				self.state = TransferState::Cancelled(CancelBy::Peer);
				Vec::new()
			}
			(TransferMsg::Result(Verdict::Ok), TransferState::Verifying) => {
				self.state = TransferState::Completed;
				Vec::new()
			}
			(TransferMsg::Result(_), _) => {
				self.state = TransferState::Failed(FailureReason::PeerReported);
				Vec::new()
			}
			_ => self.fail(FailureReason::Protocol),
		}
	}

	fn pump(&mut self) -> Vec<Effect> {
		if self.reading.is_some()
			|| self.check != Check::None
			|| self.state == TransferState::Verifying
		{
			return Vec::new();
		}
		let room = self.granted.min(self.size) - self.sent;
		if room == 0 {
			return Vec::new();
		}
		let len = usize::try_from(room)
			.unwrap_or(usize::MAX)
			.min(MAX_CHUNK_BYTES);
		self.reading = Some((self.sent, len));
		vec![Effect::ReadChunk {
			offset: self.sent,
			len,
		}]
	}

	/// Result of the engine's stat/re-hash of the source.
	pub fn source_checked(&mut self, unchanged: bool) -> Vec<Effect> {
		let phase = std::mem::replace(&mut self.check, Check::None);
		if self.state.is_terminal() || phase == Check::None {
			return Vec::new();
		}
		if !unchanged {
			return self.fail(FailureReason::SourceChanged);
		}
		match phase {
			Check::Start => {
				if self.size == 0 {
					return self.finish();
				}
				self.pump()
			}
			_ => self.finish(),
		}
	}

	fn finish(&mut self) -> Vec<Effect> {
		self.state = TransferState::Verifying;
		vec![Effect::Send(TransferMsg::Done)]
	}

	/// Data the engine read for the pending `ReadChunk`.
	pub fn chunk_read(&mut self, data: Vec<u8>) -> Vec<Effect> {
		let Some((offset, len)) = self.reading.take() else {
			return Vec::new();
		};
		if self.state.is_terminal() {
			return Vec::new();
		}
		if data.len() != len {
			return self.fail(FailureReason::SourceChanged);
		}
		self.sent = offset + len as u64;
		self.state = TransferState::Transferring;
		let mut fx = vec![Effect::Send(TransferMsg::Chunk { offset, data })];
		if self.sent == self.size {
			self.check = Check::Final;
			fx.push(Effect::CheckSource);
		} else {
			fx.extend(self.pump());
		}
		fx
	}

	/// The source could not be read.
	pub fn read_failed(&mut self) -> Vec<Effect> {
		self.reading = None;
		if self.state.is_terminal() {
			Vec::new()
		} else {
			self.fail(FailureReason::ReadError)
		}
	}
}
