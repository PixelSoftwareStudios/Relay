//! Consistent database snapshots used for recovery.

use std::fs::{self, OpenOptions};
use std::path::Path;

use relay_io::fs::{backup_path, restrict_file, sync_dir, with_suffix};
use rusqlite::Connection;

use crate::error::{ConfigError, Result};

/// Writes `<db>.bak` atomically: `VACUUM INTO` a temp file, fsync, rename.
pub(super) fn snapshot(conn: &Connection, db: &Path) -> Result<()> {
	let bak = backup_path(db);
	let tmp = with_suffix(&bak, ".tmp");
	let _ = fs::remove_file(&tmp);
	let target = tmp
		.to_str()
		.ok_or_else(|| ConfigError::io(&tmp, std::io::Error::other("path is not UTF-8")))?;
	conn.execute("VACUUM INTO ?1", [target])?;
	restrict_file(&tmp)?;
	// Write access: FlushFileBuffers (sync_all) is refused on a read-only handle on Windows.
	OpenOptions::new()
		.write(true)
		.open(&tmp)
		.and_then(|f| f.sync_all())
		.map_err(|e| ConfigError::io(&tmp, e))?;
	fs::rename(&tmp, &bak).map_err(|e| ConfigError::io(&bak, e))?;
	if let Some(dir) = bak.parent() {
		sync_dir(dir);
	}
	Ok(())
}
