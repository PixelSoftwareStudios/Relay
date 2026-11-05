//! Bounded encoding of channel messages and structural validation.

use prost::Message;
use thiserror::Error;

use super::limits::{MESSAGE_MAX_BYTES, REVOCATIONS_PER_PROOF_MAX};
use super::pake::{CONFIRM_BYTES, PAKE_MESSAGE_BYTES};
use crate::proto::channel_message::Body;
use crate::proto::{ChannelJoinGrant, ChannelMessage};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WireError {
	#[error("channel message too large")]
	TooLarge,
	#[error("channel message failed to decode")]
	Decode,
	#[error("channel message is structurally invalid")]
	Invalid,
}

pub fn encode(message: &ChannelMessage) -> Vec<u8> {
	message.encode_to_vec()
}

/// Decodes and validates untrusted bytes. Size is checked before any allocation by prost.
pub fn decode(bytes: &[u8]) -> Result<ChannelMessage, WireError> {
	if bytes.len() > MESSAGE_MAX_BYTES {
		return Err(WireError::TooLarge);
	}
	let message = ChannelMessage::decode(bytes).map_err(|_| WireError::Decode)?;
	validate(&message)?;
	Ok(message)
}

fn len_is(bytes: &[u8], n: usize) -> bool {
	bytes.len() == n
}

/// Field-length and bound checks for every variant. Semantic checks are in `join`/`proof`.
pub fn validate(message: &ChannelMessage) -> Result<(), WireError> {
	let ok = match message.body.as_ref().ok_or(WireError::Invalid)? {
		Body::Proof(m) => {
			len_is(&m.channel_ref, 16)
				&& m.cert.len() <= 512
				&& m.revocations.len() <= REVOCATIONS_PER_PROOF_MAX
				&& m.revocations.iter().all(|r| r.len() <= 512)
		}
		Body::JoinStart(m) => {
			len_is(&m.channel_ref, 16)
				&& (m.invite_id.is_empty() || len_is(&m.invite_id, 8))
				&& len_is(&m.pake_message, PAKE_MESSAGE_BYTES)
		}
		Body::JoinReply(m) => {
			len_is(&m.pake_message, PAKE_MESSAGE_BYTES) && len_is(&m.confirmation, CONFIRM_BYTES)
		}
		Body::JoinConfirm(m) => len_is(&m.confirmation, CONFIRM_BYTES),
		Body::JoinGrant(m) => valid_grant(m),
		Body::JoinDenied(_) => true,
		Body::Renew(m) => len_is(&m.channel_ref, 16) && m.cert.len() <= 512,
	};
	if ok { Ok(()) } else { Err(WireError::Invalid) }
}

fn valid_grant(m: &ChannelJoinGrant) -> bool {
	m.name.len() <= 4 * relay_core::channel::NAME_MAX_CHARS
		&& len_is(&m.owner_public, 32)
		&& m.cert.len() <= 512
}
