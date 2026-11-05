//! Content-addressed avatar cache in SQLite with a hard size bound and
//! least-recently-used eviction.

use std::path::Path;
use std::sync::Arc;

use rusqlite::{OptionalExtension, params};

use super::limits::CACHE_MAX_BYTES;
use super::process::{Avatar, validate_canonical};
use crate::db::Database;
use crate::error::Result;
use crate::fsx::WriterLock;

const LOCAL_KEY: &str = "local_avatar";

/// Avatars keyed by blake3 hash; reads re-verify the hash. The local user's own
/// avatar hash is remembered and never evicted.
#[derive(Debug, Clone)]
pub struct AvatarCache {
	db: Database,
	max_bytes: u64,
}

fn hash_of(blob: Vec<u8>) -> Option<[u8; 32]> {
	blob.try_into().ok()
}

impl AvatarCache {
	/// Uses the shared database with the default bound.
	pub fn new(db: &Database) -> Self {
		Self::with_database_limit(db, CACHE_MAX_BYTES)
	}

	pub fn with_database_limit(db: &Database, max_bytes: u64) -> Self {
		Self {
			db: db.clone(),
			max_bytes,
		}
	}

	/// A standalone cache in its own database inside `dir` (tools and tests).
	pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
		Self::with_limit(dir, CACHE_MAX_BYTES)
	}

	pub fn with_limit(dir: impl AsRef<Path>, max_bytes: u64) -> Result<Self> {
		let dir = dir.as_ref();
		let lock = Arc::new(WriterLock::acquire(&dir.join("avatars.lock"))?);
		let opened = Database::open(dir.join("avatars.db"), lock)?;
		Ok(Self::with_database_limit(&opened.database, max_bytes))
	}

	/// Stores a locally processed avatar.
	pub fn put(&self, avatar: &Avatar) -> Result<()> {
		self.write(&avatar.hash, &avatar.png)
	}

	/// Validates and stores bytes received from a peer; returns their hash.
	pub fn put_untrusted(&self, bytes: &[u8]) -> Result<[u8; 32]> {
		let hash = validate_canonical(bytes)?;
		self.write(&hash, bytes)?;
		Ok(hash)
	}

	fn write(&self, hash: &[u8; 32], bytes: &[u8]) -> Result<()> {
		if bytes.len() as u64 > self.max_bytes {
			return Ok(());
		}
		self.db.with(|conn| {
			let tx = conn.transaction()?;
			tx.execute(
				"INSERT INTO avatars (hash, data, size, last_used)
				 VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(last_used), 0) + 1 FROM avatars))
				 ON CONFLICT (hash) DO UPDATE SET last_used = excluded.last_used",
				params![hash.as_slice(), bytes, bytes.len() as i64],
			)?;
			evict(&tx, self.max_bytes, hash)?;
			tx.commit()?;
			Ok(())
		})
	}

	/// Returns cached bytes if present and still matching their hash.
	pub fn get(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>> {
		self.db.with(|conn| {
			let found: Option<Vec<u8>> = conn
				.query_row(
					"SELECT data FROM avatars WHERE hash = ?1",
					[hash.as_slice()],
					|r| r.get(0),
				)
				.optional()?;
			let Some(bytes) = found else {
				return Ok(None);
			};
			if blake3::hash(&bytes).as_bytes() != hash {
				conn.execute("DELETE FROM avatars WHERE hash = ?1", [hash.as_slice()])?;
				return Ok(None);
			}
			conn.execute(
				"UPDATE avatars SET last_used = (SELECT MAX(last_used) + 1 FROM avatars)
				 WHERE hash = ?1",
				[hash.as_slice()],
			)?;
			Ok(Some(bytes))
		})
	}

	/// Total bytes of cached avatars.
	pub fn size(&self) -> Result<u64> {
		self.db.with(|conn| total(conn))
	}

	/// Remembers which cached avatar is the local user's own.
	pub fn set_local(&self, hash: &[u8; 32]) -> Result<()> {
		self.db.with(|conn| {
			conn.execute(
				"INSERT OR REPLACE INTO state (key, value) VALUES (?1, ?2)",
				params![LOCAL_KEY, hash.as_slice()],
			)?;
			Ok(())
		})
	}

	/// The local avatar hash, only if its bytes are still cached.
	pub fn local(&self) -> Result<Option<[u8; 32]>> {
		self.db.with(|conn| {
			let hash: Option<Vec<u8>> = conn
				.query_row(
					"SELECT a.hash FROM state s JOIN avatars a ON a.hash = s.value
					 WHERE s.key = ?1",
					[LOCAL_KEY],
					|r| r.get(0),
				)
				.optional()?;
			Ok(hash.and_then(hash_of))
		})
	}
}

fn total(conn: &rusqlite::Connection) -> Result<u64> {
	let sum: i64 = conn.query_row("SELECT COALESCE(SUM(size), 0) FROM avatars", [], |r| {
		r.get(0)
	})?;
	Ok(sum.max(0) as u64)
}

/// Deletes least recently used avatars until under the bound. The entry just written
/// (`keep`) and the local avatar are never removed.
fn evict(conn: &rusqlite::Connection, max_bytes: u64, keep: &[u8; 32]) -> Result<()> {
	let mut size = total(conn)?;
	if size <= max_bytes {
		return Ok(());
	}
	let mut stmt = conn.prepare(
		"SELECT hash, size FROM avatars
		 WHERE hash != ?2 AND hash NOT IN (SELECT value FROM state WHERE key = ?1)
		 ORDER BY last_used ASC",
	)?;
	let victims = stmt
		.query_map(params![LOCAL_KEY, keep.as_slice()], |r| {
			Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?))
		})?
		.collect::<rusqlite::Result<Vec<_>>>()?;
	for (hash, len) in victims {
		if size <= max_bytes {
			break;
		}
		conn.execute("DELETE FROM avatars WHERE hash = ?1", [hash])?;
		size = size.saturating_sub(len.max(0) as u64);
	}
	Ok(())
}
