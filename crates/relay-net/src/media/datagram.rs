//! Typed bodies of media-lane records.

use thiserror::Error;

use crate::limits::{CALL_ID_BYTES, UDP_RECORD_MAX_BYTES};
use crate::record::max_body_len;

pub const CHALLENGE_BYTES: usize = 16;
const AUDIO_PREFIX_BYTES: usize = CALL_ID_BYTES + 4;

/// Largest opaque payload (an RTP packet) one datagram can carry.
pub const MAX_PAYLOAD_BYTES: usize = max_body_len(UDP_RECORD_MAX_BYTES) - AUDIO_PREFIX_BYTES;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BodyError {
	#[error("payload exceeds the datagram budget")]
	TooLarge,
	#[error("body is malformed")]
	Malformed,
}

pub type CallId = [u8; CALL_ID_BYTES];

pub fn encode_audio(
	call_id: &CallId,
	generation: u32,
	payload: &[u8],
) -> Result<Vec<u8>, BodyError> {
	if payload.len() > MAX_PAYLOAD_BYTES {
		return Err(BodyError::TooLarge);
	}
	let mut body = Vec::with_capacity(AUDIO_PREFIX_BYTES + payload.len());
	body.extend_from_slice(call_id);
	body.extend_from_slice(&generation.to_be_bytes());
	body.extend_from_slice(payload);
	Ok(body)
}

pub fn decode_audio(body: &[u8]) -> Result<(CallId, u32, &[u8]), BodyError> {
	if body.len() < AUDIO_PREFIX_BYTES {
		return Err(BodyError::Malformed);
	}
	let (call_id, rest) = body.split_at(CALL_ID_BYTES);
	let (generation, payload) = rest.split_at(4);
	let call_id: CallId = call_id.try_into().map_err(|_| BodyError::Malformed)?;
	let generation = u32::from_be_bytes(generation.try_into().map_err(|_| BodyError::Malformed)?);
	Ok((call_id, generation, payload))
}

pub fn decode_probe(body: &[u8]) -> Result<[u8; CHALLENGE_BYTES], BodyError> {
	body.try_into().map_err(|_| BodyError::Malformed)
}
