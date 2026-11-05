use relay_core::channel::{ChannelName, NameError, Policy, Role};
use thiserror::Error;

use super::cert::Authority;
use super::ids::{channel_id, random};
use super::pake::{Credential, PakeError};
use super::store::ChannelRecord;
use crate::identity::Fingerprint;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CreateError {
	#[error(transparent)]
	Name(#[from] NameError),
	#[error(transparent)]
	Password(#[from] PakeError),
	#[error("a password is required exactly when the policy asks for one")]
	PasswordMismatch,
}

/// Creates a channel owned by this device. Generates the owner signing key, derives the
/// self-certifying id, stretches the password (Argon2id, slow by design) and gives the owner
/// its own certificate.
pub fn new_channel(
	name: &str,
	policy: Policy,
	password: Option<&str>,
	local: &Fingerprint,
	now: u64,
) -> Result<ChannelRecord, CreateError> {
	let name = ChannelName::new(name)?;
	if policy.password != password.is_some() {
		return Err(CreateError::PasswordMismatch);
	}
	let authority = Authority::owner(random::<32>());
	let owner_public = authority.public();
	let id = channel_id(&owner_public);
	let password = password
		.map(|p| Credential::from_password(p, &id))
		.transpose()?;
	let mut record = ChannelRecord {
		id,
		name,
		policy,
		role: Role::Owner,
		owner_public,
		authority: Some(authority),
		password,
		cert: None,
		issued: Vec::new(),
		revocations: Vec::new(),
		invites: Vec::new(),
	};
	record.refresh_own_cert(local, now);
	Ok(record)
}
