//! The block list: devices whose calls and probes are refused.
//!
//! One table, `blocked`, rows `(fingerprint, display_name, blocked_at)`. The fingerprint is
//! the digest of the device's key, which is also its peer id, so a blocked device stays
//! blocked whatever name it uses. The size is bounded by [`MAX_BLOCKED`].

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;

use crate::db::Database;
use crate::error::{ConfigError, Result};

/// Most devices a user can block.
pub const MAX_BLOCKED: usize = 256;

/// Longest stored display name, in bytes.
const NAME_MAX: usize = 128;

/// One blocked device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedDevice {
	/// SHA-256 digest of the device's key.
	pub fingerprint: [u8; 32],
	/// The name it had when it was blocked.
	pub display_name: String,
}

/// The blocked table.
#[derive(Debug, Clone)]
pub struct Blocklist {
	db: Database,
}

impl Blocklist {
	/// Wraps the shared database.
	pub fn open(db: &Database) -> Self {
		Self { db: db.clone() }
	}

	/// Every blocked device, oldest first.
	pub fn list(&self) -> Result<Vec<BlockedDevice>> {
		self.db.with(|conn| {
			let mut stmt = conn.prepare(
				"SELECT fingerprint, display_name FROM blocked ORDER BY blocked_at, fingerprint",
			)?;
			let rows =
				stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
			let mut found = Vec::new();
			for row in rows {
				let (hex, display_name) = row?;
				if let Some(fingerprint) = hex::decode(hex).ok().and_then(|b| b.try_into().ok()) {
					found.push(BlockedDevice {
						fingerprint,
						display_name,
					});
				}
			}
			Ok(found)
		})
	}

	/// Blocks a device; blocking it again only refreshes its name. Refused when the list
	/// is full.
	pub fn add(&self, fingerprint: &[u8; 32], display_name: &str) -> Result<()> {
		let key = hex::encode(fingerprint);
		let name: String = display_name
			.chars()
			.scan(0, |bytes, c| {
				*bytes += c.len_utf8();
				(*bytes <= NAME_MAX).then_some(c)
			})
			.collect();
		let now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_or(0, |d| d.as_secs() as i64);
		self.db.with(|conn| {
			let tx = conn.transaction()?;
			let known: bool = tx.query_row(
				"SELECT EXISTS (SELECT 1 FROM blocked WHERE fingerprint = ?1)",
				[&key],
				|r| r.get(0),
			)?;
			let count: i64 = tx.query_row("SELECT COUNT(*) FROM blocked", [], |r| r.get(0))?;
			if !known && count as usize >= MAX_BLOCKED {
				return Err(ConfigError::Blocklist(format!(
					"blocked limit reached: {MAX_BLOCKED}"
				)));
			}
			tx.execute(
				"INSERT INTO blocked (fingerprint, display_name, blocked_at) VALUES (?1, ?2, ?3)
				 ON CONFLICT (fingerprint) DO UPDATE SET display_name = excluded.display_name",
				params![key, name, now],
			)?;
			tx.commit()?;
			Ok(())
		})?;
		self.db.refresh_backup(false);
		Ok(())
	}

	/// Unblocks a device; true when it was blocked.
	pub fn remove(&self, fingerprint: &[u8; 32]) -> Result<bool> {
		let removed = self.db.with(|conn| {
			Ok(conn.execute(
				"DELETE FROM blocked WHERE fingerprint = ?1",
				[hex::encode(fingerprint)],
			)? > 0)
		})?;
		if removed {
			self.db.refresh_backup(false);
		}
		Ok(removed)
	}
}
