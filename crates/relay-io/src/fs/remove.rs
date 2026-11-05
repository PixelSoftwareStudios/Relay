//! Deletion helpers.

use std::fs;
use std::path::Path;

use crate::error::{IoError, Result};

/// Deletes a file; a file that is already gone is success.
pub fn remove_if_exists(path: &Path) -> Result<()> {
	match fs::remove_file(path) {
		Ok(()) => Ok(()),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
		Err(e) => Err(IoError::io(path, e)),
	}
}
