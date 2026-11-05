//! Shared transfer vocabulary.

use crate::message::OfferStatus;

/// Why a transfer failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureReason {
	/// Received data does not hash to the offered value.
	HashMismatch,
	/// Fewer or more bytes than offered.
	SizeMismatch,
	/// The sender's file changed while sending.
	SourceChanged,
	/// Not enough free space.
	NoSpace,
	/// The destination refused or failed a write.
	Sink,
	/// The source could not be read.
	ReadError,
	/// The peer sent something invalid or unexpected.
	Protocol,
	/// The peer reported a failure.
	PeerReported,
}

/// Who cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelBy {
	/// This side.
	Local,
	/// The peer.
	Peer,
}

/// Transfer lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
	/// Offered, no answer yet.
	Offered,
	/// Receiver said no.
	Rejected,
	/// Accepted, no data yet.
	Accepted,
	/// Data flowing.
	Transferring,
	/// All bytes moved; verifying.
	Verifying,
	/// Verified and visible.
	Completed,
	/// Failed.
	Failed(FailureReason),
	/// Cancelled.
	Cancelled(CancelBy),
}

impl TransferState {
	/// True once nothing more can happen.
	pub fn is_terminal(self) -> bool {
		matches!(
			self,
			Self::Rejected | Self::Completed | Self::Failed(_) | Self::Cancelled(_)
		)
	}

	/// True while the transfer counts toward the concurrency limit.
	pub fn is_active(self) -> bool {
		matches!(self, Self::Accepted | Self::Transferring | Self::Verifying)
	}

	/// The transcript status for this state.
	pub fn offer_status(self) -> OfferStatus {
		match self {
			Self::Offered => OfferStatus::Offered,
			Self::Accepted | Self::Transferring | Self::Verifying => OfferStatus::InProgress,
			Self::Completed => OfferStatus::Completed,
			Self::Rejected => OfferStatus::Rejected,
			Self::Failed(_) => OfferStatus::Failed,
			Self::Cancelled(_) => OfferStatus::Cancelled,
		}
	}
}

/// Receiver's verdict, sent back to the sender.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
	/// Hash verified, file committed.
	Ok,
	/// Hash did not match.
	HashMismatch,
	/// Size did not match.
	SizeMismatch,
	/// Other failure.
	Failed,
}

/// Transfer messages between the two machines (control and data link).
#[derive(Clone, PartialEq, Eq)]
pub enum TransferMsg {
	/// Receiver accepts.
	Accept,
	/// Receiver refuses.
	Reject,
	/// Receiver grants bytes `[0, n)`.
	Credit(u64),
	/// File bytes at `offset` (bulk class).
	Chunk {
		/// Offset of the first byte.
		offset: u64,
		/// Data, at most 64 KiB.
		data: Vec<u8>,
	},
	/// Sender: every byte has been sent.
	Done,
	/// Receiver: outcome.
	Result(Verdict),
	/// Either side.
	Cancel,
}

impl std::fmt::Debug for TransferMsg {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Chunk { offset, data } => write!(f, "Chunk({offset}, {} bytes)", data.len()),
			Self::Accept => f.write_str("Accept"),
			Self::Reject => f.write_str("Reject"),
			Self::Credit(n) => write!(f, "Credit({n})"),
			Self::Done => f.write_str("Done"),
			Self::Result(v) => write!(f, "Result({v:?})"),
			Self::Cancel => f.write_str("Cancel"),
		}
	}
}

/// Something the engine must do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
	/// Send to the peer (`Chunk` on the bulk data link, the rest on the control link).
	Send(TransferMsg),
	/// Sender: read `len` bytes at `offset` and call `chunk_read`.
	ReadChunk {
		/// Offset.
		offset: u64,
		/// Length.
		len: usize,
	},
	/// Sender: stat/re-hash the source and call `source_checked`.
	CheckSource,
}
