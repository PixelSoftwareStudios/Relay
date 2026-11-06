//! Persistent trust store: which peer keys are TOFU-pinned or user-verified.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::error::{ConfigError, Result};

/// Most devices the trust table holds.
pub const MAX_ENTRIES: usize = 4096;
const MAX_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
	/// Pinned on first use; not compared out of band.
	Tofu,
	/// The user compared the safety code out of band.
	Verified,
}

/// One pinned peer. Keys are lowercase hex of the 32-byte public key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustEntry {
	pub public_key: String,
	pub display_name: String,
	pub level: TrustLevel,
	/// Unix seconds when first pinned.
	pub first_seen: u64,
}

/// Outcome of presenting a peer key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustDecision {
	/// Previously unseen key; caller may pin it.
	Unknown,
	Known(TrustLevel),
}

fn key_hex(key: &[u8; 32]) -> String {
	hex::encode(key)
}

impl TrustLevel {
	fn as_sql(self) -> &'static str {
		match self {
			Self::Tofu => "tofu",
			Self::Verified => "verified",
		}
	}

	fn from_sql(text: &str) -> Option<Self> {
		match text {
			"tofu" => Some(Self::Tofu),
			"verified" => Some(Self::Verified),
			_ => None,
		}
	}
}

/// Inserts or replaces one entry.
fn insert_entry(conn: &Connection, entry: &TrustEntry) -> Result<()> {
	conn.execute(
		"INSERT OR REPLACE INTO trust (public_key, display_name, level, first_seen)
		 VALUES (?1, ?2, ?3, ?4)",
		params![
			entry.public_key,
			entry.display_name,
			entry.level.as_sql(),
			entry.first_seen as i64
		],
	)?;
	Ok(())
}

fn read_entries(conn: &Connection) -> Result<Vec<TrustEntry>> {
	let mut stmt = conn.prepare(
		"SELECT public_key, display_name, level, first_seen FROM trust ORDER BY first_seen, public_key",
	)?;
	let rows = stmt.query_map([], |r| {
		Ok((
			r.get::<_, String>(0)?,
			r.get::<_, String>(1)?,
			r.get::<_, String>(2)?,
			r.get::<_, i64>(3)?,
		))
	})?;
	let mut entries = Vec::new();
	for row in rows {
		let (public_key, display_name, level, first_seen) = row?;
		let level = TrustLevel::from_sql(&level)
			.ok_or_else(|| ConfigError::InvalidPreference("unknown trust level".to_owned()))?;
		entries.push(TrustEntry {
			public_key,
			display_name,
			level,
			first_seen: first_seen.max(0) as u64,
		});
	}
	Ok(entries)
}

/// Trust data backed by the `trust` table, with an in-memory copy for lookups.
/// Every change is committed (synchronous=FULL) before it becomes visible, and
/// the recovery backup is refreshed so a pinned key is not lost to later damage.
#[derive(Debug)]
pub struct TrustStore {
	db: Database,
	entries: Vec<TrustEntry>,
}

impl TrustStore {
	/// Loads the trust table. Recovery notices come from [`Database::open`].
	pub fn open(db: &Database) -> Result<Self> {
		let entries = db.with(|c| read_entries(c))?;
		Ok(Self {
			db: db.clone(),
			entries,
		})
	}

	pub fn entries(&self) -> &[TrustEntry] {
		&self.entries
	}

	pub fn lookup(&self, key: &[u8; 32]) -> TrustDecision {
		let hex = key_hex(key);
		self.entries
			.iter()
			.find(|e| e.public_key == hex)
			.map_or(TrustDecision::Unknown, |e| TrustDecision::Known(e.level))
	}

	/// Pins a new key as TOFU; an existing entry is left untouched.
	pub fn pin_tofu(&mut self, key: &[u8; 32], display_name: &str, now: u64) -> Result<()> {
		if matches!(self.lookup(key), TrustDecision::Known(_)) {
			return Ok(());
		}
		if self.entries.len() >= MAX_ENTRIES {
			return Err(ConfigError::TrustFull(MAX_ENTRIES));
		}
		let name: String = display_name
			.chars()
			.filter(|c| !c.is_control())
			.take(MAX_NAME_CHARS)
			.collect();
		let entry = TrustEntry {
			public_key: key_hex(key),
			display_name: name,
			level: TrustLevel::Tofu,
			first_seen: now,
		};
		self.db.with(|c| insert_entry(c, &entry))?;
		self.entries.push(entry);
		self.db.refresh_backup(true);
		Ok(())
	}

	/// Marks an already pinned key as verified. Returns false if unknown.
	pub fn mark_verified(&mut self, key: &[u8; 32]) -> Result<bool> {
		let hex = key_hex(key);
		let Some(index) = self.entries.iter().position(|e| e.public_key == hex) else {
			return Ok(false);
		};
		self.db.with(|c| {
			c.execute(
				"UPDATE trust SET level = 'verified' WHERE public_key = ?1",
				[&hex],
			)?;
			Ok(())
		})?;
		self.entries[index].level = TrustLevel::Verified;
		self.db.refresh_backup(true);
		Ok(true)
	}

	/// Replaces the pinned key `old` by `new` as one transaction: the new entry is verified
	/// and keeps the old name and first-seen time, the favorite (and its place in the list)
	/// moves to the new key, and the old entry goes. Nothing changes when `old` is not pinned
	/// or `new` already is. Returns the entry that was written.
	pub fn replace_key(&mut self, old: &[u8; 32], new: &[u8; 32]) -> Result<TrustEntry> {
		let (old_hex, new_hex) = (key_hex(old), key_hex(new));
		let Some(previous) = self
			.entries
			.iter()
			.find(|e| e.public_key == old_hex)
			.cloned()
		else {
			return Err(ConfigError::KeyNotPinned);
		};
		if old_hex == new_hex || self.entries.iter().any(|e| e.public_key == new_hex) {
			return Err(ConfigError::KeyPinned);
		}
		let entry = TrustEntry {
			public_key: new_hex.clone(),
			level: TrustLevel::Verified,
			..previous
		};
		self.db.with(|c| {
			let tx = c.transaction()?;
			insert_entry(&tx, &entry)?;
			tx.execute(
				"UPDATE favorites SET public_key = ?1 WHERE public_key = ?2",
				[&new_hex, &old_hex],
			)?;
			tx.execute("DELETE FROM trust WHERE public_key = ?1", [&old_hex])?;
			tx.commit()?;
			Ok(())
		})?;
		self.entries.retain(|e| e.public_key != old_hex);
		self.entries.push(entry.clone());
		self.db.refresh_backup(true);
		Ok(entry)
	}

	/// Removes a peer. Returns whether an entry existed.
	pub fn remove(&mut self, key: &[u8; 32]) -> Result<bool> {
		let hex = key_hex(key);
		let removed = self
			.db
			.with(|c| Ok(c.execute("DELETE FROM trust WHERE public_key = ?1", [&hex])? > 0))?;
		if removed {
			self.entries.retain(|e| e.public_key != hex);
			self.db.refresh_backup(true);
		}
		Ok(removed)
	}
}
