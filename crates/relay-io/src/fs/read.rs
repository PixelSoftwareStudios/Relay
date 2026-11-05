//! Reads with explicit bounds.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use crate::error::{IoError, Result};

/// Reads a file, mapping "not found" to `None`.
pub fn read_if_exists(path: &Path) -> Result<Option<Vec<u8>>> {
	match fs::read(path) {
		Ok(bytes) => Ok(Some(bytes)),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(e) => Err(IoError::io(path, e)),
	}
}

/// Reads at most `limit` bytes; a longer file is refused without being loaded whole.
pub fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>> {
	let file = File::open(path).map_err(|e| IoError::io(path, e))?;
	let mut bytes = Vec::new();
	file.take(limit.saturating_add(1))
		.read_to_end(&mut bytes)
		.map_err(|e| IoError::io(path, e))?;
	if bytes.len() as u64 > limit {
		return Err(IoError::TooLarge {
			path: path.to_path_buf(),
			limit,
		});
	}
	Ok(bytes)
}

/// [`read_limited`] decoded as UTF-8.
pub fn read_string_limited(path: &Path, limit: u64) -> Result<String> {
	String::from_utf8(read_limited(path, limit)?).map_err(|_| {
		IoError::io(
			path,
			std::io::Error::new(std::io::ErrorKind::InvalidData, "not valid UTF-8"),
		)
	})
}
