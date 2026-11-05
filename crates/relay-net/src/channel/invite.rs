//! Invite tokens: 256-bit bearer capabilities redeemed at an admitter.
//!
//! The token string is shown once to its creator. The admitter keeps only the derived PAKE
//! credential (a store copy is therefore redeemable until expiry, but is not the string).
//! The secret is never sent: the joiner uses it as the PAKE secret, so presenting it to the
//! wrong peer discloses nothing. Single-use and expiry are enforced by the store.

use relay_core::channel::ChannelId;
use thiserror::Error;
use zeroize::Zeroizing;

use super::ids::random;
use super::limits::{INVITE_TTL_MAX, INVITE_USES_MAX};
use super::pake::Credential;
use crate::discovery::FingerprintHint;
use crate::identity::Fingerprint;

const PREFIX: &str = "RLYI1";
const CHECK_DOMAIN: &str = "relay-net 2026 invite token checksum v1";
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const BODY_BYTES: usize = 16 + 8 + 32 + 10;
const TOKEN_BYTES: usize = BODY_BYTES + 4;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InviteError {
	#[error("invite token is malformed")]
	Malformed,
	#[error("invite lifetime or use count out of range")]
	OutOfRange,
	#[error("channel already has the maximum number of active invites")]
	TooMany,
	#[error("invite is unknown, expired or already used")]
	Unusable,
}

/// What the creator hands to the invitee.
pub struct InviteToken {
	pub channel: ChannelId,
	pub id: [u8; 8],
	secret: Zeroizing<[u8; 32]>,
	/// Leading bytes of the issuer's fingerprint, to find the admitter among discovered peers.
	pub issuer_hint: [u8; 10],
}

impl InviteToken {
	pub fn credential(&self) -> Credential {
		Credential::from_invite_secret(&self.secret, &self.id, &self.channel)
	}

	pub fn issuer_hint(&self) -> FingerprintHint {
		let mut full = [0u8; 32];
		full[..10].copy_from_slice(&self.issuer_hint);
		FingerprintHint::of(&Fingerprint::from_bytes(full))
	}

	/// Whether an authenticated fingerprint is consistent with the issuer hint.
	pub fn issuer_matches(&self, fingerprint: &Fingerprint) -> bool {
		fingerprint.as_bytes()[..10] == self.issuer_hint
	}

	pub fn encode(&self) -> String {
		let mut bytes = Vec::with_capacity(TOKEN_BYTES);
		bytes.extend_from_slice(self.channel.as_bytes());
		bytes.extend_from_slice(&self.id);
		bytes.extend_from_slice(&self.secret[..]);
		bytes.extend_from_slice(&self.issuer_hint);
		let check = checksum(&bytes);
		bytes.extend_from_slice(&check);
		format!("{PREFIX}{}", base32_encode(&bytes))
	}

	/// Parses user-typed text: case-insensitive, spaces and hyphens ignored.
	pub fn parse(text: &str) -> Result<Self, InviteError> {
		let cleaned: String = text
			.chars()
			.filter(|c| !matches!(c, ' ' | '-' | '\n' | '\t'))
			.collect::<String>()
			.to_ascii_uppercase();
		let body = cleaned.strip_prefix(PREFIX).ok_or(InviteError::Malformed)?;
		let bytes = base32_decode(body).ok_or(InviteError::Malformed)?;
		if bytes.len() != TOKEN_BYTES {
			return Err(InviteError::Malformed);
		}
		let (body, check) = bytes.split_at(BODY_BYTES);
		if checksum(body) != check {
			return Err(InviteError::Malformed);
		}
		Ok(Self {
			channel: ChannelId::from_bytes(body[..16].try_into().expect("fixed slice")),
			id: body[16..24].try_into().expect("fixed slice"),
			secret: Zeroizing::new(body[24..56].try_into().expect("fixed slice")),
			issuer_hint: body[56..66].try_into().expect("fixed slice"),
		})
	}
}

impl std::fmt::Debug for InviteToken {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "InviteToken({})", self.channel)
	}
}

/// What the admitter stores for one outstanding invite.
#[derive(Clone)]
pub struct InviteRecord {
	pub id: [u8; 8],
	credential: [u8; 32],
	pub expires_at: u64,
	pub uses_left: u8,
}

impl InviteRecord {
	pub fn credential(&self) -> Credential {
		Credential::from_bytes(self.credential)
	}

	pub fn is_usable(&self, now: u64) -> bool {
		self.uses_left > 0 && now < self.expires_at
	}

	/// Rebuilds a record from stored fields (persistence layers).
	pub fn from_parts(id: [u8; 8], credential: [u8; 32], expires_at: u64, uses_left: u8) -> Self {
		Self {
			id,
			credential,
			expires_at,
			uses_left,
		}
	}

	pub fn credential_bytes(&self) -> Zeroizing<[u8; 32]> {
		Zeroizing::new(self.credential)
	}
}

impl std::fmt::Debug for InviteRecord {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "InviteRecord(uses {})", self.uses_left)
	}
}

/// Creates a token and the record the admitter keeps. `uses` is 1..=8, `ttl_secs` is
/// 1..=7 days.
pub fn create(
	channel: ChannelId,
	issuer: &Fingerprint,
	now: u64,
	ttl_secs: u64,
	uses: u8,
) -> Result<(InviteToken, InviteRecord), InviteError> {
	if uses == 0 || uses > INVITE_USES_MAX || ttl_secs == 0 || ttl_secs > INVITE_TTL_MAX.as_secs() {
		return Err(InviteError::OutOfRange);
	}
	let token = InviteToken {
		channel,
		id: random::<8>(),
		secret: Zeroizing::new(random::<32>()),
		issuer_hint: issuer.as_bytes()[..10].try_into().expect("fixed slice"),
	};
	let record = InviteRecord {
		id: token.id,
		credential: *token.credential().to_bytes(),
		expires_at: now.saturating_add(ttl_secs),
		uses_left: uses,
	};
	Ok((token, record))
}

fn checksum(body: &[u8]) -> [u8; 4] {
	let mut hasher = blake3::Hasher::new_derive_key(CHECK_DOMAIN);
	hasher.update(body);
	hasher.finalize().as_bytes()[..4]
		.try_into()
		.expect("fixed slice")
}

fn base32_encode(bytes: &[u8]) -> String {
	let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
	let (mut buffer, mut bits) = (0u32, 0u32);
	for &byte in bytes {
		buffer = (buffer << 8) | u32::from(byte);
		bits += 8;
		while bits >= 5 {
			bits -= 5;
			out.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
		}
		buffer &= (1 << bits) - 1;
	}
	if bits > 0 {
		out.push(char::from(ALPHABET[((buffer << (5 - bits)) & 31) as usize]));
	}
	out
}

fn base32_decode(text: &str) -> Option<Vec<u8>> {
	let mut out = Vec::with_capacity(text.len() * 5 / 8);
	let (mut buffer, mut bits) = (0u32, 0u32);
	for c in text.bytes() {
		let value = ALPHABET.iter().position(|&a| a == c)? as u32;
		buffer = (buffer << 5) | value;
		bits += 5;
		if bits >= 8 {
			bits -= 8;
			out.push((buffer >> bits) as u8);
			buffer &= (1 << bits) - 1;
		}
	}
	// Leftover bits are padding and must be zero, so each token has one canonical spelling.
	(buffer == 0).then_some(out)
}
