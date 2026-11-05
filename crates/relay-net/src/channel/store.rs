//! Persistence boundary for channel state. `relay-net` defines the trait; the configuration
//! layer implements it (ideally over an encrypted store or the OS credential store, because a
//! record holds PAKE-equivalent secrets and, on owner devices, the signing key).

use relay_io::LockExt;
use std::collections::BTreeMap;
use std::sync::Mutex;

use relay_core::channel::{CHANNELS_MAX, ChannelEntry, ChannelId, ChannelName, Policy, Role};
use thiserror::Error;

use super::cert::{Authority, MembershipCert, Revocation};
use super::invite::{self, InviteError, InviteRecord, InviteToken};
use super::limits::{CERT_TTL_DEFAULT, INVITES_ACTIVE_MAX, MEMBERS_ISSUED_MAX, REVOCATIONS_MAX};
use super::pake::Credential;
use crate::identity::Fingerprint;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StoreError {
	#[error("channel limit reached")]
	Full,
	#[error("channel not found")]
	NotFound,
	#[error("storage failure: {0}")]
	Backend(String),
}

/// A certificate an admitter issued, tracked so the member count stays bounded and so a
/// member can be revoked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedCert {
	pub subject: Fingerprint,
	pub expires_at: u64,
}

/// Everything this device keeps about one channel. `Debug` prints no secrets.
#[derive(Clone)]
pub struct ChannelRecord {
	pub id: ChannelId,
	pub name: ChannelName,
	pub policy: Policy,
	pub role: Role,
	pub owner_public: [u8; 32],
	/// Present on the owner and on delegated admins only.
	pub authority: Option<Authority>,
	/// Stretched password credential, kept by admitters of password channels.
	pub password: Option<Credential>,
	/// This device's own membership certificate.
	pub cert: Option<MembershipCert>,
	pub issued: Vec<IssuedCert>,
	pub revocations: Vec<Revocation>,
	pub invites: Vec<InviteRecord>,
}

impl std::fmt::Debug for ChannelRecord {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ChannelRecord")
			.field("id", &self.id)
			.field("name", &self.name.as_str())
			.field("role", &self.role)
			.finish_non_exhaustive()
	}
}

impl ChannelRecord {
	pub fn can_admit(&self) -> bool {
		self.authority.is_some()
	}

	pub fn entry(&self) -> ChannelEntry {
		ChannelEntry {
			name: self.name.clone(),
			policy: self.policy,
			role: self.role,
		}
	}

	/// Drops expired invites, issued records and revocations.
	pub fn purge(&mut self, now: u64) {
		self.invites.retain(|i| i.is_usable(now));
		self.issued.retain(|i| now < i.expires_at);
		self.revocations.retain(|r| now < r.forget_at);
	}

	pub fn is_revoked(&self, subject: &Fingerprint, now: u64) -> bool {
		self.revocations.iter().any(|r| r.covers(subject, now))
	}

	/// Stores a revocation the caller already verified; the soonest-to-lapse entry is dropped
	/// when the list is full.
	pub fn add_revocation(&mut self, revocation: Revocation) {
		if self.revocations.contains(&revocation) {
			return;
		}
		if self.revocations.len() >= REVOCATIONS_MAX
			&& let Some(oldest) = self
				.revocations
				.iter()
				.enumerate()
				.min_by_key(|(_, r)| r.forget_at)
				.map(|(i, _)| i)
		{
			self.revocations.swap_remove(oldest);
		}
		self.revocations.push(revocation);
	}

	/// Spends one use of an invite if it exists, is unexpired and has uses left.
	pub fn consume_invite(&mut self, invite: &[u8; 8], now: u64) -> bool {
		let Some(slot) = self
			.invites
			.iter_mut()
			.find(|i| &i.id == invite && i.is_usable(now))
		else {
			return false;
		};
		slot.uses_left -= 1;
		self.invites.retain(|i| i.uses_left > 0);
		true
	}

	/// Creates an invite; returns the token for the creator to hand out.
	pub fn new_invite(
		&mut self,
		issuer: &Fingerprint,
		now: u64,
		ttl_secs: u64,
		uses: u8,
	) -> Result<InviteToken, InviteError> {
		self.purge(now);
		if !self.can_admit() || !self.policy.invite_only {
			return Err(InviteError::Unusable);
		}
		if self.invites.len() >= INVITES_ACTIVE_MAX {
			return Err(InviteError::TooMany);
		}
		let (token, record) = invite::create(self.id, issuer, now, ttl_secs, uses)?;
		self.invites.push(record);
		Ok(token)
	}

	/// Issues a certificate to `subject`, tracking it. `None` when the member bound is hit,
	/// the subject is revoked, or this device cannot sign.
	pub fn admit(&mut self, subject: &Fingerprint, now: u64) -> Option<MembershipCert> {
		self.purge(now);
		let authority = self.authority.as_ref()?;
		if self.is_revoked(subject, now) {
			return None;
		}
		let existing = self.issued.iter().position(|i| &i.subject == subject);
		if existing.is_none() && self.issued.len() >= MEMBERS_ISSUED_MAX {
			return None;
		}
		let cert = authority.issue(&self.id, subject, now, CERT_TTL_DEFAULT.as_secs());
		let issued = IssuedCert {
			subject: *subject,
			expires_at: cert.expires_at,
		};
		match existing {
			Some(i) => self.issued[i] = issued,
			None => self.issued.push(issued),
		}
		Some(cert)
	}

	/// Authority holders vouch for themselves: keep their own certificate fresh.
	pub fn refresh_own_cert(&mut self, local: &Fingerprint, now: u64) {
		let stale = self
			.cert
			.as_ref()
			.is_none_or(|c| c.expires_at.saturating_sub(now) < 86_400);
		if stale && let Some(authority) = &self.authority {
			self.cert = Some(authority.issue(&self.id, local, now, CERT_TTL_DEFAULT.as_secs()));
		}
	}
}

pub trait ChannelStore: Send + Sync {
	fn list(&self) -> Result<Vec<ChannelRecord>, StoreError>;
	fn get(&self, id: &ChannelId) -> Result<Option<ChannelRecord>, StoreError>;
	/// Inserts or replaces. A new channel beyond [`CHANNELS_MAX`] is [`StoreError::Full`].
	fn put(&self, record: ChannelRecord) -> Result<(), StoreError>;
	/// Removes a channel and every secret of it.
	fn remove(&self, id: &ChannelId) -> Result<bool, StoreError>;
	/// Atomic read-modify-write of one record. `apply` runs exactly once while no other writer
	/// can interleave (a database implementation uses one transaction), so a single-use invite
	/// cannot be redeemed twice. Returns [`StoreError::NotFound`] for an unknown channel.
	fn update(
		&self,
		id: &ChannelId,
		apply: &mut dyn FnMut(&mut ChannelRecord),
	) -> Result<(), StoreError>;
}

#[derive(Default)]
pub struct InMemoryChannelStore {
	records: Mutex<BTreeMap<ChannelId, ChannelRecord>>,
}

impl InMemoryChannelStore {
	pub fn new() -> Self {
		Self::default()
	}

	fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<ChannelId, ChannelRecord>> {
		self.records.lock_unpoisoned()
	}
}

impl ChannelStore for InMemoryChannelStore {
	fn list(&self) -> Result<Vec<ChannelRecord>, StoreError> {
		Ok(self.lock().values().cloned().collect())
	}

	fn get(&self, id: &ChannelId) -> Result<Option<ChannelRecord>, StoreError> {
		Ok(self.lock().get(id).cloned())
	}

	fn put(&self, record: ChannelRecord) -> Result<(), StoreError> {
		let mut records = self.lock();
		if !records.contains_key(&record.id) && records.len() >= CHANNELS_MAX {
			return Err(StoreError::Full);
		}
		records.insert(record.id, record);
		Ok(())
	}

	fn remove(&self, id: &ChannelId) -> Result<bool, StoreError> {
		Ok(self.lock().remove(id).is_some())
	}

	fn update(
		&self,
		id: &ChannelId,
		apply: &mut dyn FnMut(&mut ChannelRecord),
	) -> Result<(), StoreError> {
		let mut records = self.lock();
		apply(records.get_mut(id).ok_or(StoreError::NotFound)?);
		Ok(())
	}
}
