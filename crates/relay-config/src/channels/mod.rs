//! Generic, protocol-agnostic storage for named channels.
//!
//! Non-secret metadata lives in SQLite; secrets (signing keys, password and invite
//! credentials) live in a [`SecretVault`](crate::vault::SecretVault) and never touch the
//! database. This module knows nothing about the channel protocol: certificates,
//! delegations and revocations are opaque blobs, flags and roles are small integers the
//! caller defines.
//!
//! Consistency across the two stores uses a manifest (`channel_secrets`) with
//! generations: new secret versions are recorded as `pending`, written to the vault,
//! then the database transaction commits them and marks superseded versions `dead`;
//! dead and pending entries are wiped from the vault afterwards and again at open.
//! A crash at any point leaves no orphan secret and never loses a committed one.

mod rows;
mod store;

use std::collections::BTreeMap;

pub use zeroize::Zeroizing;

pub use store::ChannelStorage;

const BLOB_MAX_BYTES: usize = 4096;
const NAME_MAX_BYTES: usize = 256;
const SECRET_MAX_BYTES: usize = 1024;
const SECRETS_MAX: usize = 64;
const SLOT_MAX_BYTES: usize = 40;
const ITEMS_MAX: usize = 1024;

/// A certificate the local admitter issued: tracked for counting and revocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedRow {
	pub subject: [u8; 32],
	pub expires_at: u64,
}

/// Non-secret metadata of one outstanding invite (its credential is a vault secret).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteMeta {
	pub id: [u8; 8],
	pub expires_at: u64,
	pub uses_left: u8,
}

/// Everything about a channel that may be stored in the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRow {
	pub id: [u8; 16],
	pub name: String,
	pub flags: u8,
	pub role: u8,
	pub owner_public: [u8; 32],
	/// Public delegation blob of an admin, if any.
	pub delegation: Option<Vec<u8>>,
	/// This device's own (public) membership certificate.
	pub cert: Option<Vec<u8>>,
	pub issued: Vec<IssuedRow>,
	pub revocations: Vec<Vec<u8>>,
	pub invites: Vec<InviteMeta>,
}

/// A channel row plus its secrets by slot name. `Debug` never prints secrets.
#[derive(Clone)]
pub struct ChannelData {
	pub row: ChannelRow,
	pub secrets: BTreeMap<String, Zeroizing<Vec<u8>>>,
}

impl std::fmt::Debug for ChannelData {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ChannelData")
			.field("name", &self.row.name)
			.field("secret_slots", &self.secrets.keys().collect::<Vec<_>>())
			.finish_non_exhaustive()
	}
}

impl ChannelData {
	/// Size and name checks applied before anything is written.
	pub(crate) fn validate(&self) -> crate::Result<()> {
		let bad = |m: &str| Err(crate::ConfigError::InvalidPreference(m.to_owned()));
		let row = &self.row;
		let too_big = |b: &Vec<u8>| b.len() > BLOB_MAX_BYTES;
		if row.name.len() > NAME_MAX_BYTES
			|| row.delegation.as_ref().is_some_and(too_big)
			|| row.cert.as_ref().is_some_and(too_big)
			|| row.revocations.iter().any(too_big)
		{
			return bad("channel field too large");
		}
		if row.issued.len() > ITEMS_MAX
			|| row.revocations.len() > ITEMS_MAX
			|| row.invites.len() > ITEMS_MAX
			|| self.secrets.len() > SECRETS_MAX
		{
			return bad("too many channel items");
		}
		for (slot, secret) in &self.secrets {
			crate::vault::validate_slot(slot)?;
			if slot.len() > SLOT_MAX_BYTES {
				return bad("secret slot name too long");
			}
			if secret.is_empty() || secret.len() > SECRET_MAX_BYTES {
				return bad("channel secret has an unacceptable size");
			}
		}
		Ok(())
	}
}
