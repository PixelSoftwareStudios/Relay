//! The single supported Noise composition. There is deliberately no negotiation: a peer that
//! wants another pattern or cipher fails the handshake rather than downgrading it.

use crate::limits::EPOCH_BYTES;
use crate::session::LinkEpoch;

const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE_DOMAIN: &[u8] = b"RelayPeer/v1\0";

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
pub(crate) fn prologue(major: u16, epoch: &LinkEpoch) -> Vec<u8> {
	let mut out = Vec::with_capacity(PROLOGUE_DOMAIN.len() + 2 + EPOCH_BYTES);
	out.extend_from_slice(PROLOGUE_DOMAIN);
	out.extend_from_slice(&major.to_be_bytes());
	out.extend_from_slice(epoch.as_bytes());
	out
}
