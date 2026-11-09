//! Deleting Relay's local database without the daemon.
//!
//! For a device whose daemon refuses to start (an old database it cannot open), the shell
//! needs a way out that does not depend on the daemon. [`reset_local_data`] removes the
//! preferences database, the avatar cache and every sidecar or backup of them, and nothing
//! else: the identity key (system secure storage or the identity file) is never touched, and
//! neither is any process.
//!
//! It refuses while a daemon holds the single-writer lock, and it holds that lock itself while
//! it deletes, so a daemon cannot open a half-deleted database in between.

use std::fs;
use std::path::{Path, PathBuf};

use relay_io::IoError;
use relay_io::lock::ProcessLock;
use relay_io::paths::UserDirs;

/// The files whose names (with any `-wal`, `.bak`, `.corrupt`... suffix) are deleted.
const DATABASES: [&str; 2] = ["relay.db", "avatars.db"];

/// The single-writer lock file the daemon holds inside the config directory.
const WRITER_LOCK: &str = "writer.lock";

/// Why a reset did not (fully) happen.
#[derive(Debug, thiserror::Error)]
pub enum ResetError {
	/// A daemon (or another Relay process) holds the writer lock; nothing was deleted.
	#[error("a Relay daemon is running and holds the database")]
	DaemonRunning,
	/// The directories could not be resolved or listed; nothing was deleted.
	#[error("{0}")]
	Unavailable(String),
	/// Some files could not be removed; `removed` were, `failed` were not (with the reason).
	#[error("{} file(s) could not be removed", failed.len())]
	Incomplete {
		removed: Vec<PathBuf>,
		failed: Vec<(PathBuf, String)>,
	},
}

/// [`reset_local_data`] on the platform directories of the current user.
pub fn reset_platform() -> Result<Vec<PathBuf>, ResetError> {
	let dirs = UserDirs::platform().map_err(|e| ResetError::Unavailable(e.to_string()))?;
	reset_local_data(dirs.config_dir(), dirs.data_dir())
}

/// Deletes the database, the avatar cache and their sidecar and backup files from
/// `data_dir`. `config_dir` holds the writer lock. Returns the removed files; a missing or
/// empty directory is a success with nothing removed. Files that are not Relay's database
/// (the identity key, ringtone, anything unrelated) are left alone.
pub fn reset_local_data(config_dir: &Path, data_dir: &Path) -> Result<Vec<PathBuf>, ResetError> {
	let _lock = take_lock(config_dir)?;
	let victims = list(data_dir)?;
	let mut removed = Vec::new();
	let mut failed = Vec::new();
	for path in victims {
		match fs::remove_file(&path) {
			Ok(()) => removed.push(path),
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
			Err(e) => failed.push((path, e.to_string())),
		}
	}
	if failed.is_empty() {
		Ok(removed)
	} else {
		Err(ResetError::Incomplete { removed, failed })
	}
}

/// The writer lock, or `None` when no lock file exists (no daemon ever ran here).
fn take_lock(config_dir: &Path) -> Result<Option<ProcessLock>, ResetError> {
	let file = config_dir.join(WRITER_LOCK);
	if !file.exists() {
		return Ok(None);
	}
	match ProcessLock::acquire(&file) {
		Ok(lock) => Ok(Some(lock)),
		Err(IoError::Locked(_)) => Err(ResetError::DaemonRunning),
		Err(e) => Err(ResetError::Unavailable(e.to_string())),
	}
}

fn is_victim(name: &str) -> bool {
	DATABASES.iter().any(|base| {
		name.strip_prefix(base)
			.is_some_and(|rest| rest.is_empty() || rest.starts_with(['-', '.']))
	})
}

fn list(data_dir: &Path) -> Result<Vec<PathBuf>, ResetError> {
	let entries = match fs::read_dir(data_dir) {
		Ok(entries) => entries,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
		Err(e) => {
			return Err(ResetError::Unavailable(format!(
				"{}: {e}",
				data_dir.display()
			)));
		}
	};
	let mut files = Vec::new();
	for entry in entries.flatten() {
		let is_file = entry.file_type().is_ok_and(|t| !t.is_dir());
		if is_file && entry.file_name().to_str().is_some_and(is_victim) {
			files.push(entry.path());
		}
	}
	files.sort();
	Ok(files)
}
