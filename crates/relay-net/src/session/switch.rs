//! Validation of `EpochSwitch` messages. A switch names the epoch being replaced and the one
//! that replaces it; it does not itself move any keys. The new epoch is a fresh handshake.

use thiserror::Error;

use super::LinkEpoch;
use crate::limits::{ACTIVE_CALLS_MAX, CALL_ID_BYTES};
use crate::proto::EpochSwitch;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SwitchError {
	#[error("epoch identifiers must be 16 bytes")]
	BadEpoch,
	#[error("too many or malformed call ids")]
	BadCalls,
	#[error("previous epoch is not the current epoch")]
	NotCurrent,
	#[error("next epoch equals the previous epoch")]
	NoChange,
}

/// A validated switch request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchRequest {
	pub previous: LinkEpoch,
	pub next: LinkEpoch,
	pub active_call_ids: Vec<[u8; CALL_ID_BYTES]>,
	pub acknowledge: bool,
}

/// Builds the message announcing that `next` replaces `previous` for the listed calls.
pub fn announce(
	previous: &LinkEpoch,
	next: &LinkEpoch,
	calls: &[[u8; CALL_ID_BYTES]],
) -> EpochSwitch {
	EpochSwitch {
		previous_epoch: previous.as_bytes().to_vec(),
		next_epoch: next.as_bytes().to_vec(),
		active_call_ids: calls.iter().map(|c| c.to_vec()).collect(),
		acknowledge: false,
	}
}

/// Shape-only validation used by the envelope validator.
pub(crate) fn validate_shape(message: &EpochSwitch) -> Result<SwitchRequest, SwitchError> {
	let previous = LinkEpoch::from_slice(&message.previous_epoch).ok_or(SwitchError::BadEpoch)?;
	let next = LinkEpoch::from_slice(&message.next_epoch).ok_or(SwitchError::BadEpoch)?;
	if message.active_call_ids.len() > ACTIVE_CALLS_MAX {
		return Err(SwitchError::BadCalls);
	}
	let mut calls = Vec::with_capacity(message.active_call_ids.len());
	for id in &message.active_call_ids {
		let id: [u8; CALL_ID_BYTES] = id
			.as_slice()
			.try_into()
			.map_err(|_| SwitchError::BadCalls)?;
		if calls.contains(&id) {
			return Err(SwitchError::BadCalls);
		}
		calls.push(id);
	}
	Ok(SwitchRequest {
		previous,
		next,
		active_call_ids: calls,
		acknowledge: message.acknowledge,
	})
}

/// Full validation against the epoch currently in use on this link.
pub fn validate(message: &EpochSwitch, current: &LinkEpoch) -> Result<SwitchRequest, SwitchError> {
	let request = validate_shape(message)?;
	if &request.previous != current {
		return Err(SwitchError::NotCurrent);
	}
	if request.next == request.previous {
		return Err(SwitchError::NoChange);
	}
	Ok(request)
}
