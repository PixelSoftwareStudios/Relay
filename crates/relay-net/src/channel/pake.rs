//! SPAKE2 (Ed25519 group) inside the authenticated link, with explicit key confirmation.
//!
//! The password is never used as a key or MAC key. A passive observer sees only the PAKE
//! messages; an active attacker gets one guess per run, which [`super::limiter`] bounds. The
//! `spake2` key is not trusted on its own: both sides prove knowledge of it with MACs over a
//! transcript that covers the session binding, channel id, invite id and both messages.

use argon2::{Algorithm, Argon2, Params, Version};
use relay_core::channel::ChannelId;
use spake2::{Ed25519Group, Identity, Password, Spake2};
use thiserror::Error;
use zeroize::Zeroizing;

use super::binding::SessionBinding;
use super::ids::{ct_eq, random};
use super::limits::{PASSWORD_MAX_BYTES, PASSWORD_MIN_BYTES};

const PW_SALT_DOMAIN: &str = "relay-net 2026 channel password salt v1";
const OPEN_DOMAIN: &str = "relay-net 2026 channel open credential v1";
const INVITE_DOMAIN: &str = "relay-net 2026 channel invite credential v1";
const COMBINE_DOMAIN: &str = "relay-net 2026 channel combined credential v1";
const IDENTITY_DOMAIN: &str = "relay-net 2026 channel pake identity v1";
const TRANSCRIPT_DOMAIN: &str = "relay-net 2026 channel pake transcript v1";

/// Length of a SPAKE2 message: one side byte plus a 32-byte point.
pub const PAKE_MESSAGE_BYTES: usize = 33;
pub const CONFIRM_BYTES: usize = 32;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PakeError {
	#[error("password must be {PASSWORD_MIN_BYTES} to {PASSWORD_MAX_BYTES} bytes")]
	PasswordLength,
	#[error("password stretching failed")]
	Stretch,
	#[error("malformed PAKE message")]
	Malformed,
	#[error("PAKE exchange failed")]
	Failed,
}

/// The secret a joiner must know. Always 32 bytes; zeroized on drop.
#[derive(Clone)]
pub struct Credential(Zeroizing<[u8; 32]>);

impl Credential {
	/// Credential for a channel with no password and no invite: knowing the id is enough. The
	/// value is public, so the exchange only binds the session and the id.
	pub fn open(id: &ChannelId) -> Self {
		Self(Zeroizing::new(blake3::derive_key(
			OPEN_DOMAIN,
			id.as_bytes(),
		)))
	}

	/// Stretches a human password with Argon2id (19 MiB, 2 passes, 1 lane), salted by the
	/// channel id. Run once by the joiner; admitters store only the result.
	pub fn from_password(password: &str, id: &ChannelId) -> Result<Self, PakeError> {
		if !(PASSWORD_MIN_BYTES..=PASSWORD_MAX_BYTES).contains(&password.len()) {
			return Err(PakeError::PasswordLength);
		}
		let params = Params::new(19 * 1024, 2, 1, Some(32)).map_err(|_| PakeError::Stretch)?;
		let salt = blake3::derive_key(PW_SALT_DOMAIN, id.as_bytes());
		let mut out = Zeroizing::new([0u8; 32]);
		Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
			.hash_password_into(password.as_bytes(), &salt[..16], &mut *out)
			.map_err(|_| PakeError::Stretch)?;
		Ok(Self(out))
	}

	/// Credential for a high-entropy invite secret.
	pub fn from_invite_secret(secret: &[u8; 32], invite_id: &[u8; 8], id: &ChannelId) -> Self {
		let mut hasher = blake3::Hasher::new_derive_key(INVITE_DOMAIN);
		hasher.update(secret);
		hasher.update(invite_id);
		hasher.update(id.as_bytes());
		Self(Zeroizing::new(*hasher.finalize().as_bytes()))
	}

	/// Both a password and an invite are required.
	pub fn combine(&self, other: &Self) -> Self {
		let mut hasher = blake3::Hasher::new_derive_key(COMBINE_DOMAIN);
		hasher.update(&*self.0);
		hasher.update(&*other.0);
		Self(Zeroizing::new(*hasher.finalize().as_bytes()))
	}

	/// Unguessable stand-in used for unknown channels and invites, so a failed lookup runs the
	/// same exchange and is indistinguishable from a wrong secret.
	pub fn dummy() -> Self {
		Self(Zeroizing::new(random::<32>()))
	}

	pub fn from_bytes(bytes: [u8; 32]) -> Self {
		Self(Zeroizing::new(bytes))
	}

	pub fn to_bytes(&self) -> Zeroizing<[u8; 32]> {
		Zeroizing::new(*self.0)
	}
}

impl std::fmt::Debug for Credential {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("Credential(..)")
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PakeRole {
	/// The joiner (SPAKE2 side A).
	Client,
	/// The admitter (SPAKE2 side B).
	Server,
}

/// Everything the exchange is bound to besides the secret.
#[derive(Debug, Clone, Copy)]
pub struct PakeContext {
	pub binding: SessionBinding,
	pub channel: ChannelId,
	pub invite_id: Option<[u8; 8]>,
}

impl PakeContext {
	fn identity(&self, side: u8) -> Identity {
		let mut hasher = blake3::Hasher::new_derive_key(IDENTITY_DOMAIN);
		hasher.update(&[side]);
		hasher.update(self.binding.digest());
		hasher.update(self.channel.as_bytes());
		Identity::new(hasher.finalize().as_bytes())
	}

	fn transcript(&self, msg_a: &[u8], msg_b: &[u8]) -> blake3::Hash {
		let mut hasher = blake3::Hasher::new_derive_key(TRANSCRIPT_DOMAIN);
		hasher.update(self.binding.digest());
		hasher.update(self.channel.as_bytes());
		match &self.invite_id {
			Some(id) => hasher.update(&[1]).update(id),
			None => hasher.update(&[0]),
		};
		hasher
			.update(&(msg_a.len() as u32).to_le_bytes())
			.update(msg_a);
		hasher
			.update(&(msg_b.len() as u32).to_le_bytes())
			.update(msg_b);
		hasher.finalize()
	}
}

/// One side of an in-progress exchange.
pub struct PakeState {
	spake: Spake2<Ed25519Group>,
	role: PakeRole,
	own_message: Vec<u8>,
	context: PakeContext,
}

/// Starts an exchange; returns the state and the message to send.
pub fn start(
	role: PakeRole,
	context: &PakeContext,
	credential: &Credential,
) -> (PakeState, Vec<u8>) {
	let password = Password::new(*credential.0);
	let (id_a, id_b) = (context.identity(b'A'), context.identity(b'B'));
	let (spake, message) = match role {
		PakeRole::Client => Spake2::<Ed25519Group>::start_a(&password, &id_a, &id_b),
		PakeRole::Server => Spake2::<Ed25519Group>::start_b(&password, &id_a, &id_b),
	};
	let state = PakeState {
		spake,
		role,
		own_message: message.clone(),
		context: *context,
	};
	(state, message)
}

/// Directional confirmation values derived after the peer's message arrived.
pub struct Confirmations {
	send: [u8; CONFIRM_BYTES],
	expect: [u8; CONFIRM_BYTES],
}

impl Confirmations {
	/// The MAC to send to the peer. The server sends its value in its reply; the client sends
	/// its value only after [`Self::verify`] succeeded on the server's.
	pub const fn to_send(&self) -> &[u8; CONFIRM_BYTES] {
		&self.send
	}

	/// Constant-time check of the peer's confirmation.
	pub fn verify(&self, received: &[u8]) -> bool {
		ct_eq(&self.expect, received)
	}
}

impl PakeState {
	pub fn finish(self, peer_message: &[u8]) -> Result<Confirmations, PakeError> {
		if peer_message.len() != PAKE_MESSAGE_BYTES {
			return Err(PakeError::Malformed);
		}
		let raw = Zeroizing::new(
			self.spake
				.finish(peer_message)
				.map_err(|_| PakeError::Failed)?,
		);
		let key: Zeroizing<[u8; 32]> = Zeroizing::new(
			raw.as_slice()
				.get(..32)
				.and_then(|k| k.try_into().ok())
				.ok_or(PakeError::Failed)?,
		);
		let (msg_a, msg_b) = match self.role {
			PakeRole::Client => (self.own_message.as_slice(), peer_message),
			PakeRole::Server => (peer_message, self.own_message.as_slice()),
		};
		let transcript = self.context.transcript(msg_a, msg_b);
		let mac = |label: &[u8]| -> [u8; CONFIRM_BYTES] {
			let mut hasher = blake3::Hasher::new_keyed(&key);
			hasher.update(label);
			hasher.update(transcript.as_bytes());
			*hasher.finalize().as_bytes()
		};
		let (conf_a, conf_b) = (mac(b"confirm A"), mac(b"confirm B"));
		Ok(match self.role {
			PakeRole::Client => Confirmations {
				send: conf_a,
				expect: conf_b,
			},
			PakeRole::Server => Confirmations {
				send: conf_b,
				expect: conf_a,
			},
		})
	}
}
