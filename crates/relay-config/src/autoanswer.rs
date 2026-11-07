//! Auto-answer: the contacts whose incoming calls are answered without a tap.
//!
//! One table, `auto_answer`, rows `(public_key)`. The key references the `trust` row, so
//! forgetting a peer, blocking it or replacing its pinned key deletes the flag in the same
//! statement. Only a contact whose trust level is `verified` can be added; the engine
//! checks the level again when a call arrives.

use crate::db::Database;
use crate::error::{ConfigError, Result};

/// Reason shown when the contact is not verified.
pub const NOT_VERIFIED: &str = "Only verified contacts can answer automatically";

/// The auto-answer table.
#[derive(Debug, Clone)]
pub struct AutoAnswer {
	db: Database,
}

impl AutoAnswer {
	/// Wraps the shared database.
	pub fn open(db: &Database) -> Self {
		Self { db: db.clone() }
	}

	/// The public keys flagged for auto-answer.
	pub fn list(&self) -> Result<Vec<[u8; 32]>> {
		self.db.with(|conn| {
			let mut stmt =
				conn.prepare("SELECT public_key FROM auto_answer ORDER BY public_key")?;
			let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
			let mut keys = Vec::new();
			for row in rows {
				if let Some(key) = hex::decode(row?).ok().and_then(|b| b.try_into().ok()) {
					keys.push(key);
				}
			}
			Ok(keys)
		})
	}

	/// Turns the flag on or off. Turning it on is refused, with nothing changed, unless the
	/// peer is pinned with the `verified` level.
	pub fn set(&self, key: &[u8; 32], on: bool) -> Result<()> {
		let hex = hex::encode(key);
		self.db.with(|conn| {
			if on {
				let verified: bool = conn.query_row(
					"SELECT EXISTS (SELECT 1 FROM trust WHERE public_key = ?1 AND level = 'verified')",
					[&hex],
					|r| r.get(0),
				)?;
				if !verified {
					return Err(ConfigError::Favorites(NOT_VERIFIED.to_owned()));
				}
				conn.execute(
					"INSERT OR IGNORE INTO auto_answer (public_key) VALUES (?1)",
					[&hex],
				)?;
			} else {
				conn.execute("DELETE FROM auto_answer WHERE public_key = ?1", [&hex])?;
			}
			Ok(())
		})?;
		self.db.refresh_backup(false);
		Ok(())
	}
}
