//! Favorites: the ordered quick-call list of up to [`MAX_FAVORITES`] trusted peers.
//!
//! One table, `favorites`, rows `(public_key, position)`. The key references the `trust`
//! row, so forgetting a peer (or removing its trust any other way) deletes its favorite in
//! the same statement. The count, the duplicates and the trusted-only rule are checked here
//! before anything is written; the whole list is replaced in one transaction.

use rusqlite::params;

use crate::db::Database;
use crate::error::{ConfigError, Result};

/// Most favorites a user can have.
pub const MAX_FAVORITES: usize = 6;

/// Reason shown when the list would grow past [`MAX_FAVORITES`].
pub const FULL: &str = "You can have up to 6 favorites";

/// Reason shown when a favorite is not a trusted peer.
pub const NOT_TRUSTED: &str = "Only trusted people can be favorites";

/// Reason shown when the same peer is listed twice.
pub const DUPLICATE: &str = "That person is already a favorite";

/// The favorites table.
#[derive(Debug, Clone)]
pub struct Favorites {
	db: Database,
}

impl Favorites {
	/// Wraps the shared database.
	pub fn open(db: &Database) -> Self {
		Self { db: db.clone() }
	}

	/// The favorites' public keys, in order.
	pub fn list(&self) -> Result<Vec<[u8; 32]>> {
		self.db.with(|conn| {
			let mut stmt = conn.prepare("SELECT public_key FROM favorites ORDER BY position")?;
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

	/// Replaces the list by `ordered`. Refused, with the list untouched, when it is longer
	/// than [`MAX_FAVORITES`], names a peer twice or names a peer that is not trusted.
	pub fn set(&self, ordered: &[[u8; 32]]) -> Result<()> {
		if ordered.len() > MAX_FAVORITES {
			return Err(ConfigError::Favorites(FULL.to_owned()));
		}
		let hexes: Vec<String> = ordered.iter().map(hex::encode).collect();
		for (index, key) in hexes.iter().enumerate() {
			if hexes[..index].contains(key) {
				return Err(ConfigError::Favorites(DUPLICATE.to_owned()));
			}
		}
		self.db.with(|conn| {
			let tx = conn.transaction()?;
			for key in &hexes {
				let trusted: bool = tx.query_row(
					"SELECT EXISTS (SELECT 1 FROM trust WHERE public_key = ?1)",
					[key],
					|r| r.get(0),
				)?;
				if !trusted {
					return Err(ConfigError::Favorites(NOT_TRUSTED.to_owned()));
				}
			}
			tx.execute("DELETE FROM favorites", [])?;
			for (position, key) in hexes.iter().enumerate() {
				tx.execute(
					"INSERT INTO favorites (public_key, position) VALUES (?1, ?2)",
					params![key, position as i64],
				)?;
			}
			tx.commit()?;
			Ok(())
		})?;
		self.db.refresh_backup(false);
		Ok(())
	}
}
