//! Opening everything persistent in one call.

use std::sync::Arc;

use crate::db::Database;
use crate::error::Result;
use crate::fsx::WriterLock;
use crate::notice::Notice;
use crate::paths::Paths;

/// The per-user database plus the notices raised while opening it.
///
/// Open stores from it: `PreferenceStore::open(&storage.database)`,
/// `TrustStore::open(&storage.database)`, `AvatarCache::new(&storage.database)`.
#[derive(Debug)]
pub struct Storage {
	pub database: Database,
	pub lock: Arc<WriterLock>,
	/// Recovery notices the UI must show.
	pub notices: Vec<Notice>,
}

impl Storage {
	/// Takes the writer lock, opens (recovering if needed) the database.
	pub fn open(paths: &Paths) -> Result<Self> {
		let lock = Arc::new(WriterLock::acquire(&paths.writer_lock_file())?);
		Self::open_with_lock(paths, lock)
	}

	/// Like [`Storage::open`] with a lock the caller already holds.
	pub fn open_with_lock(paths: &Paths, lock: Arc<WriterLock>) -> Result<Self> {
		let opened = Database::open(paths.database_file(), Arc::clone(&lock))?;
		Ok(Self {
			database: opened.database,
			lock,
			notices: opened.notices,
		})
	}
}
