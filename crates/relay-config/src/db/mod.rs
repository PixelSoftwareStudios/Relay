//! The per-user SQLite database: open, recover, migrate, back up.
//!
//! One connection behind a mutex, WAL journal, `synchronous=FULL`, foreign keys on.
//! Holding a [`Database`] holds the process-wide [`WriterLock`].

mod backup;
mod migrate;
mod open;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rusqlite::Connection;

pub use migrate::SCHEMA_VERSION;

use crate::error::Result;
use crate::fsx::WriterLock;
use crate::notice::Notice;

/// Minimum spacing of non-forced backup refreshes.
const BACKUP_INTERVAL: Duration = Duration::from_secs(300);

#[derive(Debug)]
struct Inner {
	conn: Mutex<Connection>,
	path: PathBuf,
	last_backup: Mutex<Option<Instant>>,
	_lock: Arc<WriterLock>,
}

/// Shared handle to the database; clones refer to the same connection.
#[derive(Debug, Clone)]
pub struct Database {
	inner: Arc<Inner>,
}

/// A freshly opened database plus anything the user must be told about.
#[derive(Debug)]
pub struct Opened {
	pub database: Database,
	pub notices: Vec<Notice>,
}

fn guard<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
	mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Database {
	/// Opens (creating if needed) the database at `path`.
	///
	/// A damaged file is preserved at `<path>.corrupt` and replaced from the backup
	/// (microphone cleared, receive-only) or, failing that, by an empty database;
	/// both cases are reported as notices. A database from a newer build is refused.
	pub fn open(path: impl Into<PathBuf>, lock: Arc<WriterLock>) -> Result<Opened> {
		let path = path.into();
		let (conn, notices) = open::open_or_recover(&path)?;
		let database = Self {
			inner: Arc::new(Inner {
				conn: Mutex::new(conn),
				path,
				last_backup: Mutex::new(None),
				_lock: lock,
			}),
		};
		database.refresh_backup(true);
		Ok(Opened { database, notices })
	}

	/// Runs `f` with exclusive use of the connection.
	pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
		f(&mut guard(&self.inner.conn))
	}

	pub fn path(&self) -> &Path {
		&self.inner.path
	}

	/// Refreshes the `.bak` snapshot. Unforced calls are rate limited. Failures are
	/// ignored: a missing backup never blocks the write that triggered it.
	pub fn refresh_backup(&self, force: bool) {
		let mut last = guard(&self.inner.last_backup);
		if !force && last.is_some_and(|t| t.elapsed() < BACKUP_INTERVAL) {
			return;
		}
		if backup::snapshot(&guard(&self.inner.conn), &self.inner.path).is_ok() {
			*last = Some(Instant::now());
		}
	}
}
