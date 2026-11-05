//! Why a channel request did not succeed, and how that is shown to clients.

use relay_api::pb::Problem;
use thiserror::Error;

/// A channel request outcome other than success.
///
/// `Denied` is deliberately uniform: an unknown channel, a wrong secret, a bad or
/// spent invite, a refusal by every reachable admitter and a non-owner asking for an
/// invite all look the same. Only a lockout differs, because the user must wait.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Failure {
	#[error("channel request denied")]
	Denied,
	#[error("too many attempts; retry in {retry_after_secs} s")]
	LockedOut { retry_after_secs: u64 },
	#[error("channel limit reached")]
	Limit,
	#[error("channel name is not acceptable")]
	NameInvalid,
	/// A malformed request (becomes `INVALID_ARGUMENT`, not an operation).
	#[error("invalid argument: {0}")]
	Invalid(&'static str),
	/// The store failed (becomes a failed precondition; the cause is logged, not sent).
	#[error("channel storage is unavailable")]
	Storage,
}

impl Failure {
	/// The stable problem for a failed operation; `None` when the request itself was
	/// malformed or storage failed, which are API errors.
	pub fn problem(&self) -> Option<Problem> {
		let (code, retryable, recovery) = match self {
			Self::Denied => ("CHANNEL_DENIED", false, ""),
			Self::LockedOut { .. } => ("CHANNEL_LOCKED_OUT", true, "wait_and_retry"),
			Self::Limit => ("CHANNEL_LIMIT", false, "leave_a_channel"),
			Self::NameInvalid => ("CHANNEL_NAME_INVALID", false, "choose_another_name"),
			Self::Invalid(_) | Self::Storage => return None,
		};
		Some(Problem {
			code: code.to_owned(),
			message: self.to_string(),
			retryable,
			recovery_action: recovery.to_owned(),
		})
	}
}
