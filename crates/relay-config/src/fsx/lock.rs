//! Single-writer lock: one process at a time may modify preferences and trust.
//!
//! Uses an OS advisory file lock, which the OS releases if the holder crashes,
//! so a stale lock file never blocks a restart.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::Path;

use crate::error::{ConfigError, Result};

/// Held for as long as the process may write configuration files.
#[derive(Debug)]
pub struct WriterLock {
	_file: File,
}

impl WriterLock {
	/// Acquires the lock or fails with [`ConfigError::Locked`] without blocking.
	pub fn acquire(path: &Path) -> Result<Self> {
		if let Some(dir) = path.parent() {
			fs::create_dir_all(dir).map_err(|e| ConfigError::io(dir, e))?;
		}
		let file = OpenOptions::new()
			.create(true)
			.truncate(false)
			.write(true)
			.open(path)
			.map_err(|e| ConfigError::io(path, e))?;
		match file.try_lock() {
			Ok(()) => Ok(Self { _file: file }),
			Err(TryLockError::WouldBlock) => Err(ConfigError::Locked(path.to_path_buf())),
			Err(TryLockError::Error(e)) => Err(ConfigError::io(path, e)),
		}
	}
}
