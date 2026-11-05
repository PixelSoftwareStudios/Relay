//! Results returned by the state owner.

use crate::effect::Effect;
use crate::error::Reject;
use crate::id::{ControllerLeaseId, InputLeaseId, Revision};
use crate::time::MonoTime;

/// Successful result payload of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
	/// Applied; nothing further to report.
	Done,
	/// A controller lease was granted (or already held).
	Controller(ControllerLeaseId),
	/// A push-to-talk lease is live until the given instant.
	Input {
		/// The lease.
		lease: InputLeaseId,
		/// When it dies unless refreshed.
		expires_at: MonoTime,
	},
}

/// What happened to a request or event.
pub type OpResult = Result<Reply, Reject>;

/// Everything one `apply` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
	/// Result of the input.
	pub result: OpResult,
	/// Revision after applying.
	pub revision: Revision,
	/// The result came from the idempotency cache; nothing was re-executed.
	pub duplicate: bool,
	/// Adapter requests, gate updates first.
	pub effects: Vec<Effect>,
}
