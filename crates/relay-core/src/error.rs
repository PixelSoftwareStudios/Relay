//! Error and rejection types.

use thiserror::Error;

use crate::call::CallState;
use crate::id::{CallId, ClientId, ControllerLeaseId, SettingsRevision};

/// A configuration value outside its permitted range.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LimitsError {
	/// The named limit is zero or exceeds its design ceiling.
	#[error("limit `{name}` is zero or above its ceiling")]
	OutOfRange {
		/// Name of the offending field.
		name: &'static str,
	},
}

/// Why a command or event was not applied.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Reject {
	/// The client is not attached.
	#[error("client is not attached")]
	UnknownClient,
	/// The client is already attached.
	#[error("client is already attached")]
	AlreadyAttached,
	/// The attached-client ceiling was reached.
	#[error("attached client limit reached")]
	ClientLimit,
	/// The operation ID was reused with a different request body.
	#[error("operation id reused with a different request")]
	OperationIdReused,
	/// Nobody is attached to this engine, so an inbound call is not rung.
	#[error("no client is attached")]
	NotAttached,
	/// The call does not exist.
	#[error("unknown call {0}")]
	UnknownCall(CallId),
	/// The call identifier is already used by a retained call or alias.
	#[error("call id {0} is already in use")]
	CallIdInUse(CallId),
	/// The active-call ceiling was reached.
	#[error("active call limit reached")]
	CallCapReached,
	/// A non-parallel call to this peer already exists.
	#[error("a call to this peer already exists: {0}")]
	SamePeerCallExists(CallId),
	/// The call already reached a terminal state.
	#[error("call already finished: {0:?}")]
	CallTerminal(CallState),
	/// The requested lifecycle action is not valid in the current state.
	#[error("action not valid while call is {state:?}")]
	InvalidState {
		/// The state that refused the action.
		state: CallState,
	},
	/// The controller lease is missing, stale or held by someone else.
	#[error("stale or missing controller lease")]
	StaleController {
		/// The lease currently in force, if any.
		current: Option<ControllerLeaseId>,
	},
	/// Another client controls the call.
	#[error("call is controlled by client {holder}")]
	ControllerHeld {
		/// The current holder.
		holder: ClientId,
	},
	/// The route description is inconsistent.
	#[error("invalid transmit route")]
	InvalidRoute,
	/// Push-to-talk is switched off (`ptt.enabled`), so it cannot be chosen.
	#[error("push-to-talk is disabled")]
	PttDisabled,
	/// The call is not active, so no transmit authority applies.
	#[error("call is not active")]
	CallNotActive,
	/// Push-to-talk was pressed without push-to-talk consent on the route.
	#[error("push-to-talk is not authorized on this call")]
	RouteNotAuthorized,
	/// The OS session (lock/suspend) blocks input.
	#[error("session blocks transmit")]
	SessionBlocked,
	/// The input sequence number is not newer than the last accepted one.
	#[error("stale input sequence, last accepted {last}")]
	StaleSequence {
		/// Last accepted sequence for this source.
		last: u64,
	},
	/// The client uses too many distinct input sources.
	#[error("too many input sources for client")]
	TooManyInputSources,
	/// The requested lease length is zero.
	#[error("invalid lease duration")]
	InvalidTtl,
	/// The release delay exceeds the hard maximum.
	#[error("release delay above the 2000 ms maximum")]
	InvalidReleaseDelay,
	/// The caller's settings revision is out of date.
	#[error("settings changed, current revision {current}")]
	SettingsConflict {
		/// The effective settings revision.
		current: SettingsRevision,
	},
	/// The completion belongs to a generation that is no longer current.
	#[error("stale generation")]
	StaleGeneration,
	/// The event does not match the call's session.
	#[error("stale or mismatched session")]
	StaleSession,
	/// A generation counter is exhausted.
	#[error("generation counter exhausted")]
	GenerationExhausted,
	/// Capture is not in a state where it can be resumed.
	#[error("capture is not awaiting resume")]
	NothingToResume,
}

/// The bounded control queue is full; the caller should back off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("control command queue is full")]
pub struct QueueFull;
