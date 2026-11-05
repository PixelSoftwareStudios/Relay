//! Consistent database snapshots used for recovery.

use std::fs::{self, File};
use std::path::Path;

use rusqlite::Connection;

use crate::error::{ConfigError, Result};
use crate::fsx::{backup_path, restrict_file, sync_dir, with_suffix};

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
	File::open(&tmp)
		.and_then(|f| f.sync_all())
		.map_err(|e| ConfigError::io(&tmp, e))?;
	fs::rename(&tmp, &bak).map_err(|e| ConfigError::io(&bak, e))?;
	if let Some(dir) = bak.parent() {
		sync_dir(dir);
	}
	Ok(())
}
