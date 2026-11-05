//! [`ChannelStore`] over `relay-config`: metadata in the per-user SQLite database,
//! secrets (owner signing key, password credential, invite credentials) in the secret
//! vault. Nothing secret reaches SQLite.

use std::collections::BTreeMap;

use relay_config::ConfigError;
use relay_config::channels::{
	ChannelData, ChannelRow, ChannelStorage, InviteMeta, IssuedRow, Zeroizing,
};
use relay_core::channel::CHANNELS_MAX;
use relay_net::channel::store::{ChannelRecord, ChannelStore, IssuedCert, StoreError};
use relay_net::channel::{
	Authority, ChannelId, ChannelName, Credential, Delegation, InviteRecord, MembershipCert,
	Policy, Revocation, Role,
};
use relay_net::identity::Fingerprint;

const SLOT_SIGNING: &str = "signing";
const SLOT_PASSWORD: &str = "password";
const INVITE_PREFIX: &str = "invite-";

/// Persistent channel store for the daemon.
#[derive(Debug, Clone)]
pub struct SqliteChannelStore {
	storage: ChannelStorage,
}

impl SqliteChannelStore {
	pub fn new(storage: ChannelStorage) -> Self {
		Self { storage }
	}
}

fn backend(e: ConfigError) -> StoreError {
	match e {
		ConfigError::ChannelLimit => StoreError::Full,
		other => StoreError::Backend(other.to_string()),
	}
}

fn corrupt(what: &str) -> StoreError {
	StoreError::Backend(format!("stored channel is malformed: {what}"))
}

fn secret(bytes: &[u8]) -> Zeroizing<Vec<u8>> {
	Zeroizing::new(bytes.to_vec())
}

fn role_code(role: Role) -> u8 {
	match role {
		Role::Owner => 0,
		Role::Admin => 1,
		Role::Member => 2,
	}
}

fn role_from(code: u8) -> Result<Role, StoreError> {
	match code {
		0 => Ok(Role::Owner),
		1 => Ok(Role::Admin),
		2 => Ok(Role::Member),
		_ => Err(corrupt("role")),
	}
}

fn flags(policy: Policy) -> u8 {
	u8::from(policy.listed) | u8::from(policy.password) << 1 | u8::from(policy.invite_only) << 2
}

fn policy_from(flags: u8) -> Policy {
	Policy {
		listed: flags & 1 != 0,
		password: flags & 2 != 0,
		invite_only: flags & 4 != 0,
	}
}

fn invite_slot(id: &[u8; 8]) -> String {
	format!("{INVITE_PREFIX}{}", hex::encode(id))
}

fn to_data(record: &ChannelRecord) -> ChannelData {
	let mut secrets = BTreeMap::new();
	let mut delegation = None;
	if let Some(authority) = &record.authority {
		secrets.insert(SLOT_SIGNING.to_owned(), secret(authority.seed()));
		delegation = authority.delegation().map(|d| d.to_bytes().to_vec());
	}
	if let Some(password) = &record.password {
		secrets.insert(SLOT_PASSWORD.to_owned(), secret(&*password.to_bytes()));
	}
	for invite in &record.invites {
		secrets.insert(invite_slot(&invite.id), secret(&*invite.credential_bytes()));
	}
	ChannelData {
		row: ChannelRow {
			id: *record.id.as_bytes(),
			name: record.name.as_str().to_owned(),
			flags: flags(record.policy),
			role: role_code(record.role),
			owner_public: record.owner_public,
			delegation,
			cert: record.cert.as_ref().map(MembershipCert::to_bytes),
			issued: record
				.issued
				.iter()
				.map(|i| IssuedRow {
					subject: *i.subject.as_bytes(),
					expires_at: i.expires_at,
				})
				.collect(),
			revocations: record
				.revocations
				.iter()
				.map(Revocation::to_bytes)
				.collect(),
			invites: record
				.invites
				.iter()
				.map(|i| InviteMeta {
					id: i.id,
					expires_at: i.expires_at,
					uses_left: i.uses_left,
				})
				.collect(),
		},
		secrets,
	}
}

fn key32(bytes: &[u8], what: &str) -> Result<[u8; 32], StoreError> {
	bytes.try_into().map_err(|_| corrupt(what))
}

fn from_data(data: &ChannelData) -> Result<ChannelRecord, StoreError> {
	let row = &data.row;
	let authority = match data.secrets.get(SLOT_SIGNING) {
		Some(seed) => {
			let seed = key32(seed, "signing key")?;
			Some(match &row.delegation {
				Some(blob) => Authority::admin(
					seed,
					Delegation::from_bytes(blob).map_err(|_| corrupt("delegation"))?,
				),
				None => Authority::owner(seed),
			})
		}
		None => None,
	};
	let password = data
		.secrets
		.get(SLOT_PASSWORD)
		.map(|b| key32(b, "password credential").map(Credential::from_bytes))
		.transpose()?;
	let invites = row
		.invites
		.iter()
		.map(|meta| {
			let credential = data
				.secrets
				.get(&invite_slot(&meta.id))
				.ok_or_else(|| corrupt("invite credential"))?;
			Ok(InviteRecord::from_parts(
				meta.id,
				key32(credential, "invite credential")?,
				meta.expires_at,
				meta.uses_left,
			))
		})
		.collect::<Result<Vec<_>, StoreError>>()?;
	Ok(ChannelRecord {
		id: ChannelId::from_bytes(row.id),
		name: ChannelName::new(&row.name).map_err(|_| corrupt("name"))?,
		policy: policy_from(row.flags),
		role: role_from(row.role)?,
		owner_public: row.owner_public,
		authority,
		password,
		cert: row
			.cert
			.as_deref()
			.map(MembershipCert::from_bytes)
			.transpose()
			.map_err(|_| corrupt("certificate"))?,
		issued: row
			.issued
			.iter()
			.map(|i| IssuedCert {
				subject: Fingerprint::from_bytes(i.subject),
				expires_at: i.expires_at,
			})
			.collect(),
		revocations: row
			.revocations
			.iter()
			.map(|b| Revocation::from_bytes(b))
			.collect::<Result<_, _>>()
			.map_err(|_| corrupt("revocation"))?,
		invites,
	})
}

impl ChannelStore for SqliteChannelStore {
	fn list(&self) -> Result<Vec<ChannelRecord>, StoreError> {
		let all = self.storage.list().map_err(backend)?;
		all.iter().map(from_data).collect()
	}

	fn get(&self, id: &ChannelId) -> Result<Option<ChannelRecord>, StoreError> {
		let found = self.storage.get(id.as_bytes()).map_err(backend)?;
		found.as_ref().map(from_data).transpose()
	}

	fn put(&self, record: ChannelRecord) -> Result<(), StoreError> {
		self.storage
			.put(&to_data(&record), CHANNELS_MAX)
			.map_err(backend)
	}

	fn remove(&self, id: &ChannelId) -> Result<bool, StoreError> {
		self.storage.remove(id.as_bytes()).map_err(backend)
	}

	fn update(
		&self,
		id: &ChannelId,
		apply: &mut dyn FnMut(&mut ChannelRecord),
	) -> Result<(), StoreError> {
		let mut failure = None;
		let found = self
			.storage
			.update(id.as_bytes(), &mut |data| match from_data(data) {
				Ok(mut record) => {
					apply(&mut record);
					*data = to_data(&record);
				}
				// Leave the stored data as it was and report the damage.
				Err(e) => failure = Some(e),
			})
			.map_err(backend)?;
		match (failure, found) {
			(Some(e), _) => Err(e),
			(None, false) => Err(StoreError::NotFound),
			(None, true) => Ok(()),
		}
	}
}
