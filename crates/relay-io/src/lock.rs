//! Advisory process lock on a file.
//!
//! The operating system releases it when the holder exits or crashes, so a stale
//! lock file never blocks a restart. Used both as the single-writer lock for
//! configuration and as the per-endpoint daemon start lock.

use std::fs::{File, TryLockError};
use std::path::Path;

use crate::error::{IoError, Result};
use crate::fs::{create_private_file, ensure_private_dir};

/// Held for as long as the value lives.
#[derive(Debug)]
pub struct ProcessLock {
	_file: File,
}

impl ProcessLock {
	/// Takes the lock without blocking, creating the file (0600) and its parent
	/// directory (0700, tightened if loose). Fails with [`IoError::Locked`] when
	/// another holder exists.
	pub fn acquire(path: &Path) -> Result<Self> {
		if let Some(dir) = path.parent() {
			ensure_private_dir(dir)?;
		}
		Self::lock(path)
	}

	/// Like [`ProcessLock::acquire`] but a missing parent is created 0700 and an
	/// existing one is left as it is (for lock files that sit beside a socket in a
	/// directory the caller does not own outright).
	pub fn acquire_beside(path: &Path) -> Result<Self> {
		if let Some(dir) = path.parent() {
			create_dir_0700(dir)?;
		}
		Self::lock(path)
	}

	fn lock(path: &Path) -> Result<Self> {
		let file = create_private_file(path)?;
		match file.try_lock() {
			Ok(()) => Ok(Self { _file: file }),
			Err(TryLockError::WouldBlock) => Err(IoError::Locked(path.to_path_buf())),
			Err(TryLockError::Error(e)) => Err(IoError::io(path, e)),
		}
	}
}

fn create_dir_0700(dir: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::DirBuilderExt;
		std::fs::DirBuilder::new()
			.recursive(true)
			.mode(0o700)
			.create(dir)
			.map_err(|e| IoError::io(dir, e))
	}
	#[cfg(not(unix))]
	std::fs::create_dir_all(dir).map_err(|e| IoError::io(dir, e))
}
