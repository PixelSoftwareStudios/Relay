//! Opening the database file, detecting damage and recovering from the backup.

use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode};

use super::migrate;
use crate::error::{ConfigError, Result};
use crate::fsx::{backup_path, corrupt_path, ensure_private_dir, restrict_file, with_suffix};
use crate::notice::Notice;
use crate::prefs::sql as prefs_sql;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Why an open attempt failed.
enum Failure {
	/// The file is not a usable database; recovery may help.
	Damaged,
	/// Recovery would not help or would be unsafe (newer schema, I/O, permissions).
	Fatal(ConfigError),
}

impl From<rusqlite::Error> for Failure {
	fn from(e: rusqlite::Error) -> Self {
		match e.sqlite_error_code() {
			Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) => Self::Damaged,
			_ => Self::Fatal(e.into()),
		}
	}
}

impl From<ConfigError> for Failure {
	fn from(e: ConfigError) -> Self {
		Self::Fatal(e)
	}
}

pub(super) fn open_or_recover(path: &Path) -> Result<(Connection, Vec<Notice>)> {
	if let Some(dir) = path.parent() {
		ensure_private_dir(dir)?;
	}
	match try_open(path, true) {
		Ok(conn) => Ok((conn, Vec::new())),
		Err(Failure::Fatal(e)) => Err(e),
		Err(Failure::Damaged) => recover(path),
	}
}

/// `strict_empty`: a missing or empty file next to a backup counts as damage.
fn try_open(path: &Path, strict_empty: bool) -> std::result::Result<Connection, Failure> {
	let len = fs::metadata(path).map(|m| m.len()).ok();
	// A missing or emptied file next to a backup is damage, not a first run.
	if strict_empty && matches!(len, None | Some(0)) && backup_path(path).exists() {
		return Err(Failure::Damaged);
	}
	if len.is_none() {
		create_private(path)?;
	}
	restrict_file(path)?;
	let mut conn = Connection::open(path)?;
	let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
	if version > i64::from(migrate::SCHEMA_VERSION) {
		return Err(ConfigError::UnsupportedVersion {
			found: version,
			supported: migrate::SCHEMA_VERSION,
		}
		.into());
	}
	let check: String = conn.pragma_query_value(None, "quick_check", |r| r.get(0))?;
	if check != "ok" {
		return Err(Failure::Damaged);
	}
	conn.busy_timeout(BUSY_TIMEOUT)?;
	conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get::<_, String>(0))?;
	conn.pragma_update(None, "synchronous", "FULL")?;
	conn.pragma_update(None, "foreign_keys", "ON")?;
	conn.pragma_update(None, "trusted_schema", "OFF")?;
	migrate::run(&mut conn, version as u32)?;
	Ok(conn)
}

fn create_private(path: &Path) -> Result<()> {
	let mut options = OpenOptions::new();
	options.write(true).create(true).truncate(false);
	#[cfg(unix)]
	{
		use std::os::unix::fs::OpenOptionsExt;
		options.mode(0o600);
	}
	options
		.open(path)
		.map(drop)
		.map_err(|e| ConfigError::io(path, e))
}

/// Moves the damaged database aside and returns where it was kept.
fn preserve(path: &Path) -> Option<std::path::PathBuf> {
	let kept = corrupt_path(path);
	let _ = fs::remove_file(&kept);
	let moved = fs::rename(path, &kept).is_ok();
	discard_sidecars(path);
	moved.then_some(kept)
}

fn discard_sidecars(path: &Path) {
	for suffix in ["-wal", "-shm"] {
		let _ = fs::remove_file(with_suffix(path, suffix));
	}
}

fn discard(path: &Path) {
	let _ = fs::remove_file(path);
	discard_sidecars(path);
}

fn recover(path: &Path) -> Result<(Connection, Vec<Notice>)> {
	let preserved = preserve(path);
	let bak = backup_path(path);
	if bak.exists() && fs::copy(&bak, path).is_ok() {
		match try_open(path, true) {
			Ok(mut conn) => {
				if prefs_sql::clear_microphone(&mut conn).is_ok() {
					return Ok((
						conn,
						vec![
							Notice::PreferencesRecoveredFromBackup,
							Notice::TrustRecoveredFromBackup,
						],
					));
				}
				drop(conn);
				discard(path);
			}
			Err(Failure::Fatal(e)) if !matches!(e, ConfigError::UnsupportedVersion { .. }) => {
				return Err(e);
			}
			Err(_) => discard(path),
		}
	}
	match try_open(path, false) {
		Ok(conn) => Ok((
			conn,
			vec![
				Notice::PreferencesReset {
					preserved: preserved.clone(),
				},
				Notice::TrustStoreLost { preserved },
			],
		)),
		Err(Failure::Fatal(e)) => Err(e),
		Err(Failure::Damaged) => Err(ConfigError::io(
			path,
			std::io::Error::other("a fresh database could not be created"),
		)),
	}
}
