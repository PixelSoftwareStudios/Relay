//! The single supported Noise composition. There is deliberately no negotiation: a peer that
//! wants another pattern or cipher fails the handshake rather than downgrading it.

use crate::limits::EPOCH_BYTES;
use crate::session::LinkEpoch;

const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE_DOMAIN: &[u8] = b"RelayPeer/v1\0";
const DATA_PROLOGUE_DOMAIN: &[u8] = b"RelayData/v1\0";

/// What a handshake is for. The prologue differs, so a control handshake can never be
/// completed as a data link (or the reverse) even with identical keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Purpose {
	Control,
	Data,
}

/// Relay peer protocol major carried in the preface and the prologue.
pub const PROTOCOL_MAJOR: u16 = 1;

pub(crate) fn builder() -> snow::Builder<'static> {
	snow::Builder::new(
		PATTERN
			.parse()
			.expect("static Noise pattern string is valid"),
	)
}

/// Prologue: domain string, major version and the fresh link epoch.
pub(crate) fn prologue(purpose: Purpose, major: u16, epoch: &LinkEpoch) -> Vec<u8> {
	let domain = match purpose {
		Purpose::Control => PROLOGUE_DOMAIN,
		Purpose::Data => DATA_PROLOGUE_DOMAIN,
	};
	let mut out = Vec::with_capacity(domain.len() + 2 + EPOCH_BYTES);
	out.extend_from_slice(domain);
	out.extend_from_slice(&major.to_be_bytes());
	out.extend_from_slice(epoch.as_bytes());
	out
}
