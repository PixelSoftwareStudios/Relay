//! Per-session facts the transcript store does not hold: the call, the transfers, the
//! ordinal base.

use std::collections::HashMap;
use std::path::PathBuf;

use relay_core::{CallId, PeerId};
use relay_messages::OfferId;
use relay_messages::SessionId;
use relay_messages::transfer::FailureReason;
use tokio::sync::mpsc;

use super::source::Facts;

/// What a running transfer task is told.
#[derive(Debug)]
pub enum XferCmd {
	/// Stop: the local user cancelled, files were disabled or the session ended.
	Cancel,
	/// The peer cancelled.
	PeerCancel,
}

/// One file transfer's bookkeeping in the actor. The state machine itself lives in the
/// task, next to the file and the data link.
pub struct Xfer {
	/// This side sends the file.
	pub outgoing: bool,
	/// Sender: the source on this machine. Never logged.
	pub source: Option<SourceInfo>,
	/// Receiver: where the file goes. Never logged.
	pub dest_dir: Option<PathBuf>,
	/// Receiver: the name suggested by the client, if any.
	pub dest_name: Option<String>,
	/// The task, once the transfer runs.
	pub task: Option<mpsc::Sender<XferCmd>>,
	/// Bytes moved so far.
	pub transferred: u64,
	/// Receiver asked for a preview, the sender has not answered.
	pub preview_requested: bool,
	/// Sender: the peer asked, the local user has not decided.
	pub preview_pending: bool,
	/// Receiver: final name on disk.
	pub saved_as: String,
	/// Why it failed or was cancelled.
	pub failure: String,
	/// Sender: accepted by the peer, waiting for the grant.
	pub accepted: bool,
	/// Counts toward the per-session concurrency limit.
	pub active: bool,
	/// Last progress event published (bytes), to rate-limit progress events.
	pub last_reported: u64,
}

/// The sender's file.
#[derive(Clone)]
pub struct SourceInfo {
	/// Absolute path as given.
	pub path: PathBuf,
	/// Facts taken while hashing.
	pub facts: Facts,
}

impl Xfer {
	/// A fresh record.
	pub fn new(outgoing: bool) -> Self {
		Self {
			outgoing,
			source: None,
			dest_dir: None,
			dest_name: None,
			task: None,
			transferred: 0,
			preview_requested: false,
			preview_pending: false,
			saved_as: String::new(),
			failure: String::new(),
			accepted: false,
			active: false,
			last_reported: 0,
		}
	}
}

/// A session of the actor.
pub struct Meta {
	/// Store key.
	pub id: SessionId,
	/// The owning call.
	pub call: CallId,
	/// The peer.
	pub peer: PeerId,
	/// The local engine started the call.
	pub host: bool,
	/// Messages removed by clears so far; ordinal = base + index.
	pub base: u64,
	/// Transfers by offer.
	pub xfers: HashMap<OfferId, Xfer>,
}

/// Lowercase label of a failure reason.
pub fn failure_label(reason: FailureReason) -> &'static str {
	match reason {
		FailureReason::HashMismatch => "hash_mismatch",
		FailureReason::SizeMismatch => "size_mismatch",
		FailureReason::SourceChanged => "source_changed",
		FailureReason::NoSpace => "no_space",
		FailureReason::Sink => "destination_error",
		FailureReason::ReadError => "read_error",
		FailureReason::Protocol => "protocol",
		FailureReason::PeerReported => "peer_reported",
	}
}
