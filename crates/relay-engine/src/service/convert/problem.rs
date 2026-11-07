//! Domain rejections and failures as client-visible problems.

use relay_api::pb::Problem;
use relay_core::Reject;

use crate::service::ports::PortError;

fn problem(code: &str, message: String, retryable: bool, recovery: &str) -> Problem {
	Problem {
		code: code.to_owned(),
		message,
		retryable,
		recovery_action: recovery.to_owned(),
	}
}

/// Maps a domain rejection to a stable machine code.
pub fn problem_of(reject: &Reject) -> Problem {
	let (code, retryable, recovery) = match reject {
		Reject::UnknownClient => ("UNKNOWN_CLIENT", false, "register"),
		Reject::AlreadyAttached => ("ALREADY_ATTACHED", false, ""),
		Reject::ClientLimit => ("CLIENT_LIMIT", true, "close_other_clients"),
		Reject::OperationIdReused => ("OPERATION_ID_REUSED", false, "use_new_operation_id"),
		Reject::UnknownCall(_) => ("UNKNOWN_CALL", false, "refresh_state"),
		Reject::CallIdInUse(_) => ("CALL_ID_IN_USE", false, ""),
		Reject::NotAttached => ("NOT_ATTACHED", true, "attach_a_client"),
		Reject::CallCapReached => ("CALL_LIMIT", true, "end_a_call"),
		Reject::SamePeerCallExists(_) => ("PEER_ALREADY_CALLED", false, "allow_parallel"),
		Reject::CallTerminal(_) => ("CALL_FINISHED", false, "refresh_state"),
		Reject::InvalidState { .. } => ("INVALID_CALL_STATE", false, "refresh_state"),
		Reject::StaleController { .. } => ("STALE_CONTROLLER", false, "acquire_controller"),
		Reject::ControllerHeld { .. } => ("CONTROLLER_HELD", false, "explicit_takeover"),
		Reject::InvalidRoute => ("INVALID_ROUTE", false, ""),
		Reject::CallNotActive => ("CALL_NOT_ACTIVE", true, "wait_for_connected"),
		Reject::PttDisabled => ("PTT_DISABLED", false, "enable_push_to_talk"),
		Reject::RouteNotAuthorized => ("ROUTE_NOT_AUTHORIZED", false, "set_push_to_talk_route"),
		Reject::SessionBlocked => ("SESSION_BLOCKED", true, "unlock"),
		Reject::StaleSequence { .. } => ("STALE_SEQUENCE", false, ""),
		Reject::TooManyInputSources => ("TOO_MANY_INPUT_SOURCES", false, ""),
		Reject::InvalidTtl => ("INVALID_LEASE", false, ""),
		Reject::InvalidReleaseDelay => ("INVALID_RELEASE_DELAY", false, ""),
		Reject::SettingsConflict { .. } => ("SETTINGS_CONFLICT", true, "refresh_state"),
		Reject::StaleGeneration => ("STALE_GENERATION", true, "refresh_state"),
		Reject::StaleSession => ("STALE_SESSION", false, "refresh_state"),
		Reject::GenerationExhausted => ("GENERATION_EXHAUSTED", false, "restart_engine"),
		Reject::NothingToResume => ("NOTHING_TO_RESUME", false, ""),
	};
	problem(code, reject.to_string(), retryable, recovery)
}

/// Maps an adapter failure to a stable machine code.
pub fn service_problem(error: &PortError) -> Problem {
	let (code, retryable) = match error {
		PortError::Unsupported(_) => ("UNSUPPORTED", false),
		PortError::Invalid(_) => ("INVALID_REQUEST", false),
		PortError::NotFound(_) => ("NOT_FOUND", false),
		PortError::Ambiguous(_) => ("PEER_AMBIGUOUS", false),
		PortError::KeyMismatch => ("TRUST_KEY_MISMATCH", false),
		PortError::Unavailable(_) => ("UNAVAILABLE", true),
		PortError::Rejected { code, .. } => (*code, false),
	};
	problem(code, error.to_string(), retryable, "")
}
