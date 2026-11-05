//! Structural validation of `relay.peer.v1` envelopes. Generated types are converted at this
//! trust boundary; nothing downstream needs to reason about missing or oversize fields.

use rand::RngExt;
use thiserror::Error;

use crate::identity::Fingerprint;
use crate::limits::{CALL_ID_BYTES, DISPLAY_NAME_MAX_BYTES, DISPLAY_NAME_MAX_CHARS, EPOCH_BYTES};
use crate::noise::PROTOCOL_MAJOR;
use crate::proto::{Envelope, Hello, envelope::Body};

const MESSAGE_ID_BYTES: usize = 16;
const AVATAR_CHUNK_MAX_BYTES: usize = 8192;
const MEDIA_PROFILES_MAX: usize = 16;
const ENDPOINT_CANDIDATES_MAX: usize = 8;
const REASON_CODE_MAX_BYTES: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct ValidationError(&'static str);

fn fail(reason: &'static str) -> ValidationError {
	ValidationError(reason)
}

pub fn new_message_id() -> Vec<u8> {
	let mut id = vec![0u8; MESSAGE_ID_BYTES];
	rand::rng().fill(&mut id[..]);
	id
}

/// Wraps a body in an envelope with a fresh unpredictable message id.
pub fn new_envelope(body: Body) -> Envelope {
	Envelope {
		message_id: new_message_id(),
		protocol_major: u32::from(PROTOCOL_MAJOR),
		reply_to_message_id: Vec::new(),
		body: Some(body),
	}
}

fn call_id(id: &[u8]) -> Result<(), ValidationError> {
	if id.len() == CALL_ID_BYTES {
		Ok(())
	} else {
		Err(fail("call id must be 16 bytes"))
	}
}

fn reason(code: &str) -> Result<(), ValidationError> {
	if code.len() <= REASON_CODE_MAX_BYTES {
		Ok(())
	} else {
		Err(fail("reason code too long"))
	}
}

pub fn validate_envelope(envelope: &Envelope) -> Result<(), ValidationError> {
	if envelope.message_id.len() != MESSAGE_ID_BYTES {
		return Err(fail("message id must be 16 bytes"));
	}
	if envelope.protocol_major != u32::from(PROTOCOL_MAJOR) {
		return Err(fail("unsupported protocol major"));
	}
	if !envelope.reply_to_message_id.is_empty()
		&& envelope.reply_to_message_id.len() != MESSAGE_ID_BYTES
	{
		return Err(fail("reply-to id must be empty or 16 bytes"));
	}
	let body = envelope
		.body
		.as_ref()
		.ok_or_else(|| fail("envelope has no body"))?;
	validate_body(body)
}

fn validate_body(body: &Body) -> Result<(), ValidationError> {
	match body {
		Body::Hello(h) => validate_hello(h),
		Body::Invite(m) => {
			call_id(&m.call_id)?;
			if m.offered_profiles.len() > MEDIA_PROFILES_MAX {
				return Err(fail("too many offered profiles"));
			}
			Ok(())
		}
		Body::Ringing(m) => call_id(&m.call_id),
		Body::Accept(m) => call_id(&m.call_id),
		Body::Reject(m) => call_id(&m.call_id).and_then(|()| reason(&m.reason_code)),
		Body::End(m) => call_id(&m.call_id).and_then(|()| reason(&m.reason_code)),
		Body::Keepalive(_) => Ok(()),
		Body::EndpointOffer(m) => {
			if m.link_epoch.len() != EPOCH_BYTES {
				return Err(fail("link epoch must be 16 bytes"));
			}
			if m.candidates.len() > ENDPOINT_CANDIDATES_MAX {
				return Err(fail("too many endpoint candidates"));
			}
			Ok(())
		}
		Body::Profile(m) => display_name(&m.display_name),
		Body::AvatarRequest(_) => Ok(()),
		Body::AvatarChunk(m) => {
			if m.data.len() > AVATAR_CHUNK_MAX_BYTES {
				return Err(fail("avatar chunk too large"));
			}
			Ok(())
		}
		Body::EpochSwitch(m) => crate::session::switch::validate_shape(m)
			.map(|_| ())
			.map_err(|_| fail("malformed epoch switch")),
		Body::Error(m) => {
			if !m.call_id.is_empty() {
				call_id(&m.call_id)?;
			}
			reason(&m.code)
		}
		Body::MediaReady(m) => {
			call_id(&m.call_id)?;
			if m.link_epoch.len() != EPOCH_BYTES {
				return Err(fail("link epoch must be 16 bytes"));
			}
			Ok(())
		}
		Body::MediaFeedback(m) => call_id(&m.call_id),
		Body::StreamState(m) => call_id(&m.call_id),
	}
}

fn display_name(name: &str) -> Result<(), ValidationError> {
	if name.len() > DISPLAY_NAME_MAX_BYTES || name.chars().count() > DISPLAY_NAME_MAX_CHARS {
		return Err(fail("display name too long"));
	}
	Ok(())
}

fn validate_hello(hello: &Hello) -> Result<(), ValidationError> {
	if hello.protocol_major != u32::from(PROTOCOL_MAJOR) {
		return Err(fail("unsupported hello major"));
	}
	if hello.public_key_sha256.len() != 32 {
		return Err(fail("hello key digest must be 32 bytes"));
	}
	if hello.engine_incarnation.len() != 16 {
		return Err(fail("engine incarnation must be 16 bytes"));
	}
	if hello.media_profiles.len() > MEDIA_PROFILES_MAX {
		return Err(fail("too many media profiles"));
	}
	display_name(&hello.display_name)
}

/// The Hello digest must equal the digest of the key authenticated by the handshake.
/// (The field keeps its schema name; the digest function is the crate's fingerprint.)
pub(super) fn check_hello(
	hello: &Hello,
	authenticated: &Fingerprint,
) -> Result<(), ValidationError> {
	if hello.public_key_sha256.as_slice() == authenticated.as_bytes() {
		Ok(())
	} else {
		Err(fail("hello digest does not match the authenticated key"))
	}
}

/// Builds a `Hello` carrying the local fingerprint.
pub fn new_hello(
	fingerprint: &Fingerprint,
	display_name: &str,
	engine_incarnation: [u8; 16],
) -> Hello {
	Hello {
		protocol_major: u32::from(PROTOCOL_MAJOR),
		protocol_minor: 0,
		public_key_sha256: fingerprint.as_bytes().to_vec(),
		display_name: display_name.to_owned(),
		media_profiles: Vec::new(),
		max_parallel_calls: 1,
		engine_incarnation: engine_incarnation.to_vec(),
	}
}
