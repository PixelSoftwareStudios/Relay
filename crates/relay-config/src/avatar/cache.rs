//! Avatar cache: content-addressed, bounded, least-recently-used and disposable.
//!
//! The bytes live in their own SQLite file (`avatars.db`, see `store`) so the settings
//! database, which is backed up often, never carries them. The only thing kept in the
//! settings database is which avatar is the local user's own (`state` table); that
//! avatar is pinned and never evicted. If the cache file is lost the local avatar
//! reads as absent until it is uploaded again.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{OptionalExtension, params};

use super::limits::CACHE_MAX_BYTES;
use super::process::{Avatar, validate_canonical};
use super::store::{self, Store};
use crate::db::Database;
use crate::error::Result;
use crate::writer_lock::WriterLock;

const LOCAL_KEY: &str = "local_avatar";
const FILE_NAME: &str = "avatars.db";

/// Avatars keyed by blake3 hash; reads re-verify the hash.
#[derive(Debug, Clone)]
pub struct AvatarCache {
	settings: Database,
	store: Arc<Mutex<Store>>,
	max_bytes: u64,
}

impl AvatarCache {
	/// Cache file `avatars.db` next to the settings database, default bound.
	pub fn new(db: &Database) -> Self {
		Self::with_database_limit(db, CACHE_MAX_BYTES)
	}

	pub fn with_database_limit(db: &Database, max_bytes: u64) -> Self {
		let file = db.path().with_file_name(FILE_NAME);
		Self::at(db, file, max_bytes)
	}

	/// Cache file at an explicit path.
	pub fn at(db: &Database, file: PathBuf, max_bytes: u64) -> Self {
		Self {
			settings: db.clone(),
			store: Arc::new(Mutex::new(Store::new(file))),
			max_bytes,
		}
	}

	/// A standalone cache in `dir` with its own small settings database (tools, tests).
	pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
		Self::with_limit(dir, CACHE_MAX_BYTES)
	}

	pub fn with_limit(dir: impl AsRef<Path>, max_bytes: u64) -> Result<Self> {
		let dir = dir.as_ref();
		let lock = Arc::new(WriterLock::acquire(&dir.join("avatars.lock"))?);
		let opened = Database::open(dir.join("relay.db"), lock)?;
		Ok(Self::with_database_limit(&opened.database, max_bytes))
	}

	fn store(&self) -> MutexGuard<'_, Store> {
		self.store.lock().unwrap_or_else(|e| e.into_inner())
	}

	fn pinned(&self) -> Result<Option<[u8; 32]>> {
		self.settings.with(|conn| {
			let value: Option<Vec<u8>> = conn
				.query_row("SELECT value FROM state WHERE key = ?1", [LOCAL_KEY], |r| {
					r.get(0)
				})
				.optional()?;
			Ok(value.and_then(|v| v.try_into().ok()))
		})
	}

	/// Stores a locally processed avatar.
	pub fn put(&self, avatar: &Avatar) -> Result<()> {
		self.write(&avatar.hash, &avatar.image)
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
		let pinned = self.pinned()?;
		self.store()
			.run(|conn| store::insert(conn, hash, bytes, pinned.as_ref(), self.max_bytes))
	}

	/// Returns cached bytes if present and still matching their hash.
	pub fn get(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>> {
		self.store().run(|conn| store::fetch(conn, hash))
	}

	/// Total bytes of cached avatars.
	pub fn size(&self) -> Result<u64> {
		self.store().run(|conn| store::total(conn))
	}

	/// Remembers (in the settings database) which avatar is the local user's own.
	pub fn set_local(&self, hash: &[u8; 32]) -> Result<()> {
		self.settings.with(|conn| {
			conn.execute(
				"INSERT OR REPLACE INTO state (key, value) VALUES (?1, ?2)",
				params![LOCAL_KEY, hash.as_slice()],
			)?;
			Ok(())
		})
	}

	/// The local avatar hash, only if its bytes are still cached.
	pub fn local(&self) -> Result<Option<[u8; 32]>> {
		let Some(hash) = self.pinned()? else {
			return Ok(None);
		};
		let present = self.store().run(|conn| store::contains(conn, &hash))?;
		Ok(present.then_some(hash))
	}
}
