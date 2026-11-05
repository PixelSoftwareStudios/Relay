//! Crash-safe file replacement.
//!
//! Sequence: write `<file>.tmp`, fsync it, copy the current file to `<file>.bak`,
//! rename the temp file over the target, fsync the directory. A crash at any step
//! leaves either the old file, the backup, or the new file readable.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{ConfigError, Result};

/// `path` with `suffix` appended to its file name.
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
	let mut name = path.as_os_str().to_owned();
	name.push(suffix);
	PathBuf::from(name)
}

/// Location of the previous good copy of `path`.
pub fn backup_path(path: &Path) -> PathBuf {
	with_suffix(path, ".bak")
}

/// Where an unreadable `path` is preserved for the user to inspect.
pub fn corrupt_path(path: &Path) -> PathBuf {
	with_suffix(path, ".corrupt")
}

/// Reads a file, mapping "not found" to `None`.
pub fn read_if_exists(path: &Path) -> Result<Option<Vec<u8>>> {
	match fs::read(path) {
		Ok(bytes) => Ok(Some(bytes)),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(e) => Err(ConfigError::io(path, e)),
	}
}

/// Atomically replaces `path` with `bytes`, keeping the previous content as backup.
///
/// `private` creates files with mode 0600 on Unix.
pub fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> Result<()> {
	let dir = path.parent().unwrap_or_else(|| Path::new("."));
	fs::create_dir_all(dir).map_err(|e| ConfigError::io(dir, e))?;

	let tmp = with_suffix(path, ".tmp");
	let mut file = open_tmp(&tmp, private).map_err(|e| ConfigError::io(&tmp, e))?;
	file.write_all(bytes)
		.map_err(|e| ConfigError::io(&tmp, e))?;
	file.sync_all().map_err(|e| ConfigError::io(&tmp, e))?;
	drop(file);

	if path.exists() {
		let bak = backup_path(path);
		fs::copy(path, &bak).map_err(|e| ConfigError::io(&bak, e))?;
		File::open(&bak)
			.and_then(|f| f.sync_all())
			.map_err(|e| ConfigError::io(&bak, e))?;
	}
	fs::rename(&tmp, path).map_err(|e| ConfigError::io(path, e))?;
	sync_dir(dir);
	Ok(())
}

fn open_tmp(tmp: &Path, private: bool) -> std::io::Result<File> {
	let mut options = OpenOptions::new();
	options.write(true).create(true).truncate(true);
	#[cfg(unix)]
	if private {
		use std::os::unix::fs::OpenOptionsExt;
		options.mode(0o600);
	}
	#[cfg(not(unix))]
	let _ = private;
	options.open(tmp)
}

/// Best-effort directory fsync so the rename itself is durable.
pub fn sync_dir(dir: &Path) {
	#[cfg(unix)]
	if let Ok(handle) = File::open(dir) {
		let _ = handle.sync_all();
	}
	#[cfg(not(unix))]
	let _ = dir;
}
