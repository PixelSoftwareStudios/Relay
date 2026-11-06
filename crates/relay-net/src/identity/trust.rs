//! Trust decisions. The store records user decisions only; a handshake never writes to it.

use std::collections::HashMap;
use std::time::SystemTime;

use thiserror::Error;

use super::Fingerprint;
use crate::discovery::name_skeleton;

const TRUST_RECORDS_MAX: usize = 1024;

/// How a key came to be trusted. The two states must be labelled distinctly in any UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustState {
	/// Trusted on first use without out-of-band comparison; impersonation at first contact
	/// was possible and is not excluded.
	UnverifiedFirstUse,
	/// The user compared the full fingerprint out of band.
	Verified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustRecord {
	pub fingerprint: Fingerprint,
	pub state: TrustState,
	/// Name shown when the key was trusted. A hint only; names are never identity.
	pub display_name: String,
	pub trusted_at: SystemTime,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TrustError {
	#[error("trust limit reached: {0}")]
	Full(usize),
	#[error("no trust record for this fingerprint")]
	Unknown,
	#[error("that key is already trusted")]
	Exists,
	#[error("could not save the device: {0}")]
	Storage(String),
}

/// Storage for trust decisions. Persistence is supplied by the configuration layer.
pub trait TrustStore {
	fn get(&self, fingerprint: &Fingerprint) -> Option<TrustRecord>;
	/// Records whose name *looks like* `display_name`: implementations compare
	/// [`crate::discovery::name_skeleton`] of both names, not the raw strings, so a lookalike
	/// (invisible character, confusable letter, case, spacing) of a trusted contact's name is
	/// found and [`evaluate_peer`] reports `KeyChanged` instead of `New`.
	fn find_by_name(&self, display_name: &str) -> Vec<TrustRecord>;
	/// Records an explicit user decision. May upgrade `UnverifiedFirstUse` to `Verified`.
	fn put(&mut self, record: TrustRecord) -> Result<(), TrustError>;
	fn remove(&mut self, fingerprint: &Fingerprint) -> Result<(), TrustError>;
}

/// Result of consulting the store after authentication. Never mutates the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerTrust {
	Verified,
	UnverifiedFirstUse,
	/// No record: a first-contact flow with fingerprint display is required.
	New,
	/// An unknown key presents a name already trusted under a different key. This is a
	/// security event; the old record is not replaced implicitly.
	KeyChanged {
		previous: Box<TrustRecord>,
	},
}

/// Classifies an authenticated peer key. `claimed_name` is the untrusted advertised name.
pub fn evaluate_peer(
	store: &dyn TrustStore,
	fingerprint: &Fingerprint,
	claimed_name: &str,
) -> PeerTrust {
	if let Some(record) = store.get(fingerprint) {
		return match record.state {
			TrustState::Verified => PeerTrust::Verified,
			TrustState::UnverifiedFirstUse => PeerTrust::UnverifiedFirstUse,
		};
	}
	match store.find_by_name(claimed_name).into_iter().next() {
		Some(previous) => PeerTrust::KeyChanged {
			previous: Box::new(previous),
		},
		None => PeerTrust::New,
	}
}

/// Bounded in-memory store.
#[derive(Debug, Default)]
pub struct InMemoryTrustStore {
	records: HashMap<Fingerprint, TrustRecord>,
}

impl InMemoryTrustStore {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn len(&self) -> usize {
		self.records.len()
	}

	pub fn is_empty(&self) -> bool {
		self.records.is_empty()
	}
}

impl TrustStore for InMemoryTrustStore {
	fn get(&self, fingerprint: &Fingerprint) -> Option<TrustRecord> {
		self.records.get(fingerprint).cloned()
	}

	fn find_by_name(&self, display_name: &str) -> Vec<TrustRecord> {
		let wanted = name_skeleton(display_name);
		self.records
			.values()
			.filter(|r| name_skeleton(&r.display_name) == wanted)
			.cloned()
			.collect()
	}

	fn put(&mut self, record: TrustRecord) -> Result<(), TrustError> {
		if !self.records.contains_key(&record.fingerprint)
			&& self.records.len() >= TRUST_RECORDS_MAX
		{
			return Err(TrustError::Full(TRUST_RECORDS_MAX));
		}
		// A verified record is never silently downgraded by a later first-use decision.
		if let Some(existing) = self.records.get(&record.fingerprint)
			&& existing.state == TrustState::Verified
			&& record.state == TrustState::UnverifiedFirstUse
		{
			return Ok(());
		}
		self.records.insert(record.fingerprint, record);
		Ok(())
	}

	fn remove(&mut self, fingerprint: &Fingerprint) -> Result<(), TrustError> {
		self.records
			.remove(fingerprint)
			.map(|_| ())
			.ok_or(TrustError::Unknown)
	}
}
