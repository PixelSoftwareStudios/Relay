//! Favorites on disk: peer ids on the API side, trust public keys in the table.

use relay_config::autoanswer::AutoAnswer;
use relay_config::db::Database;
use relay_config::favorites::Favorites;
use relay_config::identity::TrustStore;
use relay_core::PeerId;
use relay_net::identity::PublicKey;

use crate::service::ports::PortError;

/// The `favorites` and `auto_answer` tables seen through peer ids (key fingerprints).
#[derive(Debug, Clone)]
pub struct FavoriteBook {
	db: Database,
	table: Favorites,
	auto: AutoAnswer,
}

fn id_of(key: &[u8; 32]) -> PeerId {
	PeerId::from_bytes(*PublicKey::from_bytes(*key).fingerprint().as_bytes())
}

impl FavoriteBook {
	/// Wraps the shared database.
	pub fn open(db: &Database) -> Self {
		Self {
			db: db.clone(),
			table: Favorites::open(db),
			auto: AutoAnswer::open(db),
		}
	}

	/// The favorites in order; empty when the table cannot be read.
	pub fn list(&self) -> Vec<PeerId> {
		self.table
			.list()
			.map(|keys| keys.iter().map(id_of).collect())
			.unwrap_or_default()
	}

	/// Replaces the favorites. Refused with the reason when the list is too long, repeats a
	/// peer or names a peer that is not trusted; nothing changes then.
	pub fn set(&self, ordered: &[PeerId]) -> Result<(), PortError> {
		let trusted =
			TrustStore::open(&self.db).map_err(|e| PortError::Unavailable(e.to_string()))?;
		let mut keys = Vec::with_capacity(ordered.len());
		for peer in ordered {
			let found = trusted.entries().iter().find_map(|entry| {
				let key: [u8; 32] = hex::decode(&entry.public_key).ok()?.try_into().ok()?;
				(id_of(&key) == *peer).then_some(key)
			});
			// An unknown peer gets a key that is in no trust row, so the table says why.
			keys.push(found.unwrap_or([0; 32]));
		}
		self.table
			.set(&keys)
			.map_err(|e| PortError::Invalid(e.to_string()))
	}

	/// The contacts flagged for auto-answer; empty when the table cannot be read.
	pub fn auto_answer(&self) -> Vec<PeerId> {
		self.auto
			.list()
			.map(|keys| keys.iter().map(id_of).collect())
			.unwrap_or_default()
	}

	/// Flags or unflags a contact. Switching on is refused unless the contact is verified.
	pub fn set_auto_answer(&self, peer: PeerId, on: bool) -> Result<(), PortError> {
		let trusted =
			TrustStore::open(&self.db).map_err(|e| PortError::Unavailable(e.to_string()))?;
		let found = trusted.entries().iter().find_map(|entry| {
			let key: [u8; 32] = hex::decode(&entry.public_key).ok()?.try_into().ok()?;
			(id_of(&key) == peer).then_some(key)
		});
		match (found, on) {
			(Some(key), _) => self.auto.set(&key, on),
			// Switching off a contact that is gone has nothing to undo.
			(None, false) => return Ok(()),
			(None, true) => self.auto.set(&[0; 32], true),
		}
		.map_err(|e| PortError::Invalid(e.to_string()))
	}
}
