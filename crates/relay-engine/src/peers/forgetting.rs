//! Forgetting a stored trust decision: finding the peer and cutting its link.

use relay_net::identity::{Fingerprint, TrustRecord};

use super::handle::{Command, PeerError, PeerHandle, PeerSnapshot};
use super::manager::Actor;
use super::peer_id;
use crate::service::ports::{ForgottenPeer, PeerSelector, PortError};

/// Fingerprint digits needed before a prefix selects anything.
const MIN_PREFIX_DIGITS: usize = 8;
const MAX_QUERY_CHARS: usize = 128;

fn digits(query: &str) -> Option<String> {
	let text: String = query
		.chars()
		.filter(|c| !matches!(c, ' ' | '-' | ':'))
		.map(|c| c.to_ascii_lowercase())
		.collect();
	(text.len() >= MIN_PREFIX_DIGITS && text.chars().all(|c| c.is_ascii_hexdigit())).then_some(text)
}

fn name_of(record: &TrustRecord, live: &[PeerSnapshot]) -> String {
	live.iter()
		.find(|p| {
			p.id.is_some_and(|id| id.as_bytes() == record.fingerprint.as_bytes())
		})
		.map_or_else(|| record.display_name.clone(), |p| p.name.clone())
}

/// The stored record a refused (offending) key stands for, if one is remembered.
fn behind<'a>(
	records: &'a [TrustRecord],
	changes: &[(Fingerprint, Fingerprint)],
	offending: &[u8],
) -> Option<&'a TrustRecord> {
	let (_, previous) = changes.iter().find(|(o, _)| o.as_bytes() == offending)?;
	records.iter().find(|r| r.fingerprint == *previous)
}

/// Picks exactly one stored trust record. The key may be the pinned one or, after a key
/// change, the new key that was refused: both name the same contact. A fingerprint prefix
/// (8 or more hex digits, any separators) is tried before an exact, case-insensitive display
/// name. Nothing is guessed: several matches are an error that names them. `changes` pairs
/// each refused key with the pinned key it would replace.
pub(super) fn resolve(
	records: &[TrustRecord],
	live: &[PeerSnapshot],
	changes: &[(Fingerprint, Fingerprint)],
	selector: &PeerSelector,
) -> Result<ForgottenPeer, PortError> {
	let query = match selector {
		PeerSelector::Id(id) => {
			let found = records
				.iter()
				.find(|r| r.fingerprint.as_bytes() == id.as_bytes())
				.or_else(|| behind(records, changes, id.as_bytes()))
				.ok_or_else(|| PortError::NotFound("no stored trust for that peer".to_owned()))?;
			return Ok(ForgottenPeer {
				id: peer_id(&found.fingerprint),
				name: name_of(found, live),
				key_changed: Vec::new(),
			});
		}
		PeerSelector::Query(query) => query.trim(),
	};
	if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
		return Err(PortError::Invalid("name a peer".to_owned()));
	}
	let by_digits = digits(query).map(|wanted| {
		let mut found: Vec<&TrustRecord> = records
			.iter()
			.filter(|r| hex::encode(r.fingerprint.as_bytes()).starts_with(&wanted))
			.collect();
		for (offending, _) in changes {
			if hex::encode(offending.as_bytes()).starts_with(&wanted)
				&& let Some(record) = behind(records, changes, offending.as_bytes())
				&& !found.iter().any(|r| r.fingerprint == record.fingerprint)
			{
				found.push(record);
			}
		}
		found
	});
	let matches = match by_digits {
		Some(found) if !found.is_empty() => found,
		_ => {
			let wanted = query.to_lowercase();
			records
				.iter()
				.filter(|r| {
					name_of(r, live).to_lowercase() == wanted
						|| r.display_name.to_lowercase() == wanted
				})
				.collect()
		}
	};
	match matches.as_slice() {
		[] => Err(PortError::NotFound(
			"no stored trust matches that fingerprint or name".to_owned(),
		)),
		[one] => Ok(ForgottenPeer {
			id: peer_id(&one.fingerprint),
			name: name_of(one, live),
			key_changed: Vec::new(),
		}),
		many => {
			let listed: Vec<String> = many
				.iter()
				.map(|r| format!("{} ({})", name_of(r, live), r.fingerprint.short()))
				.collect();
			Err(PortError::Ambiguous(format!(
				"{} peers match: {}",
				many.len(),
				listed.join(", ")
			)))
		}
	}
}

impl PeerHandle {
	/// Every stored trust decision, linked or not.
	pub fn trust_records(&self) -> Vec<TrustRecord> {
		let trust = self.trust.lock();
		trust
			.fingerprints()
			.iter()
			.filter_map(|fingerprint| trust.get(fingerprint))
			.collect()
	}

	/// Finds the one stored peer `selector` names and forgets it: the decision is removed at
	/// once and the manager cuts the link and stops reconnecting. A device that still trusts
	/// us and calls again is treated as unknown. After a key change the refused key selects
	/// the same contact as the pinned one, and the key-change state goes with it.
	pub fn forget(&self, selector: &PeerSelector) -> Result<ForgottenPeer, PortError> {
		let changes: Vec<_> = self
			.key_changes
			.all()
			.into_iter()
			.map(|(offending, change)| (offending, change.previous))
			.collect();
		let mut found = resolve(&self.trust_records(), &self.peers(), &changes, selector)?;
		let fingerprint = Fingerprint::from_bytes(*found.id.as_bytes());
		self.trust
			.lock()
			.remove(&fingerprint)
			.map_err(|e| PortError::Unavailable(e.to_string()))?;
		let offenders = self.key_changes.clear_previous(&fingerprint);
		found.key_changed = offenders.iter().map(peer_id).collect();
		self.try_send_forgotten(fingerprint)
			.map_err(|e| PortError::Unavailable(e.to_string()))?;
		for offending in offenders {
			let _ = self.try_send_forgotten(offending);
		}
		Ok(found)
	}

	fn try_send_forgotten(&self, peer: Fingerprint) -> Result<(), PeerError> {
		self.try_send(Command::Forgotten(peer))
	}
}

impl Actor {
	/// The trust record is already gone; stop treating the link as trusted.
	pub(super) async fn forgotten(&mut self, peer: Fingerprint) {
		self.wanted.remove(&peer);
		self.pending.remove(&peer);
		if let Some(id) = self.canonical.get(&peer).copied() {
			self.drop_link(id).await;
		}
	}
}
