//! Single-writer lock: one process at a time may modify preferences and trust.

use std::path::Path;

use relay_io::lock::ProcessLock;

use crate::error::Result;

/// Held for as long as the process may write configuration files.
#[derive(Debug)]
pub struct WriterLock {
	_lock: ProcessLock,
}

impl WriterLock {
	/// Acquires the lock or fails with [`crate::ConfigError::Locked`] without blocking.
	pub fn acquire(path: &Path) -> Result<Self> {
		Ok(Self {
			_lock: ProcessLock::acquire(path)?,
		})
	}
}
