//! The avatar cache file: a disposable SQLite database of avatar bytes.
//!
//! Unlike the settings database it has no backup, no recovery notice and relaxed
//! durability. Anything wrong with it (not a database, corrupt pages, an unusable
//! schema) is answered by deleting the file and starting over, because every avatar in
//! it can be fetched from its peer again. Only real I/O problems, such as permissions,
//! surface as errors.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OptionalExtension, params};

use crate::error::{ConfigError, Result};
use crate::fsx::{ensure_private_dir, restrict_file, with_suffix};

const BUSY_TIMEOUT: Duration = Duration::from_secs(2);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS avatars (
	hash BLOB PRIMARY KEY NOT NULL CHECK (length(hash) = 32),
	data BLOB NOT NULL,
	size INTEGER NOT NULL,
	last_used INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS avatars_by_use ON avatars (last_used);
";

#[derive(Debug)]
pub(super) struct Store {
	path: PathBuf,
	conn: Option<Connection>,
}

fn is_damage(e: &rusqlite::Error) -> bool {
	matches!(
		e.sqlite_error_code(),
		Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
	)
}

fn wipe(path: &Path) {
	tracing::debug!(path = %path.display(), "avatar cache unusable; recreating it empty");
	for suffix in ["", "-journal", "-wal", "-shm"] {
		let _ = fs::remove_file(with_suffix(path, suffix));
	}
}

fn open_file(path: &Path) -> std::result::Result<Connection, rusqlite::Error> {
	let conn = Connection::open(path)?;
	conn.busy_timeout(BUSY_TIMEOUT)?;
	conn.pragma_update(None, "synchronous", "OFF")?;
	conn.pragma_update(None, "trusted_schema", "OFF")?;
	conn.execute_batch(SCHEMA)?;
	Ok(conn)
}

impl Store {
	pub(super) fn new(path: PathBuf) -> Self {
		Self { path, conn: None }
	}

	fn create_private(&self) -> Result<()> {
		if let Some(dir) = self.path.parent() {
			ensure_private_dir(dir)?;
		}
		let mut options = OpenOptions::new();
		options.write(true).create(true).truncate(false);
		#[cfg(unix)]
		{
			use std::os::unix::fs::OpenOptionsExt;
			options.mode(0o600);
		}
		options
			.open(&self.path)
			.map(drop)
			.map_err(|e| ConfigError::io(&self.path, e))?;
		restrict_file(&self.path)
	}

	fn connect(&mut self) -> Result<&mut Connection> {
		if self.conn.is_none() {
			self.create_private()?;
			let conn = match open_file(&self.path) {
				Ok(conn) => conn,
				Err(first) => {
					tracing::debug!(error = %first, "avatar cache failed to open");
					wipe(&self.path);
					self.create_private()?;
					open_file(&self.path)?
				}
			};
			// Journal and sidecar files inherit nothing from the main file's mode.
			restrict_file(&self.path)?;
			self.conn = Some(conn);
		}
		Ok(self.conn.as_mut().expect("connection was just opened"))
	}

	/// Runs `f`; on database damage wipes the cache and runs it once more on the
	/// fresh, empty file.
	pub(super) fn run<T>(
		&mut self,
		f: impl Fn(&mut Connection) -> rusqlite::Result<T>,
	) -> Result<T> {
		match f(self.connect()?) {
			Err(e) if is_damage(&e) => {
				tracing::debug!(error = %e, "avatar cache damaged");
				self.conn = None;
				wipe(&self.path);
				Ok(f(self.connect()?)?)
			}
			other => Ok(other?),
		}
	}
}

pub(super) fn total(conn: &Connection) -> rusqlite::Result<u64> {
	let sum: i64 = conn.query_row("SELECT COALESCE(SUM(size), 0) FROM avatars", [], |r| {
		r.get(0)
	})?;
	Ok(sum.max(0) as u64)
}

pub(super) fn contains(conn: &Connection, hash: &[u8; 32]) -> rusqlite::Result<bool> {
	conn.query_row(
		"SELECT 1 FROM avatars WHERE hash = ?1",
		[hash.as_slice()],
		|_| Ok(()),
	)
	.optional()
	.map(|found| found.is_some())
}

fn next_tick(conn: &Connection) -> rusqlite::Result<i64> {
	conn.query_row(
		"SELECT COALESCE(MAX(last_used), 0) + 1 FROM avatars",
		[],
		|r| r.get(0),
	)
}

/// Returns the bytes if cached and still matching their hash (tampered rows are
/// dropped), marking them most recently used.
pub(super) fn fetch(conn: &Connection, hash: &[u8; 32]) -> rusqlite::Result<Option<Vec<u8>>> {
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
		"UPDATE avatars SET last_used = ?2 WHERE hash = ?1",
		params![hash.as_slice(), next_tick(conn)?],
	)?;
	Ok(Some(bytes))
}

/// Inserts an entry after evicting least recently used ones (never `pinned`) until it
/// fits, so the bound holds at every instant. Skips the insert when it cannot fit.
pub(super) fn insert(
	conn: &mut Connection,
	hash: &[u8; 32],
	bytes: &[u8],
	pinned: Option<&[u8; 32]>,
	max_bytes: u64,
) -> rusqlite::Result<()> {
	let tx = conn.transaction()?;
	let tick = next_tick(&tx)?;
	if contains(&tx, hash)? {
		tx.execute(
			"UPDATE avatars SET last_used = ?2 WHERE hash = ?1",
			params![hash.as_slice(), tick],
		)?;
		return tx.commit();
	}
	let len = bytes.len() as u64;
	let pinned = pinned.map(|h| h.as_slice());
	let mut size = total(&tx)?;
	while size + len > max_bytes {
		let victim: Option<(Vec<u8>, i64)> = tx
			.query_row(
				"SELECT hash, size FROM avatars WHERE hash IS NOT ?1
				 ORDER BY last_used ASC LIMIT 1",
				[pinned],
				|r| Ok((r.get(0)?, r.get(1)?)),
			)
			.optional()?;
		let Some((victim, victim_size)) = victim else {
			return Ok(());
		};
		tx.execute("DELETE FROM avatars WHERE hash = ?1", [victim])?;
		size = size.saturating_sub(victim_size.max(0) as u64);
	}
	tx.execute(
		"INSERT INTO avatars (hash, data, size, last_used) VALUES (?1, ?2, ?3, ?4)",
		params![hash.as_slice(), bytes, bytes.len() as i64, tick],
	)?;
	tx.commit()
}
