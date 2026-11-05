//! Single-writer lock: one process at a time may modify preferences and trust.
//!
//! Uses an OS advisory file lock, which the OS releases if the holder crashes,
//! so a stale lock file never blocks a restart.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use super::private::ensure_private_dir;
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
			ensure_private_dir(dir)?;
		}
		let mut options = OpenOptions::new();
		options.create(true).truncate(false).write(true);
		#[cfg(unix)]
		{
			use std::os::unix::fs::OpenOptionsExt;
			options.mode(0o600);
		}
		let file = options.open(path).map_err(|e| ConfigError::io(path, e))?;
		match file.try_lock() {
			Ok(()) => Ok(Self { _file: file }),
			Err(TryLockError::WouldBlock) => Err(ConfigError::Locked(path.to_path_buf())),
			Err(TryLockError::Error(e)) => Err(ConfigError::io(path, e)),
		}
	}
}
