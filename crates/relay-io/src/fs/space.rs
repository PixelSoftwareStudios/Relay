//! Free space of the filesystem holding a directory.

use std::path::Path;

/// Bytes available to this user on the filesystem containing `dir`.
pub fn free_space(dir: &Path) -> std::io::Result<u64> {
	// Windows reports the volume of a missing path as if it existed; Unix `statvfs`
	// fails. Make both fail the same way.
	if !std::fs::metadata(dir)?.is_dir() {
		return Err(std::io::Error::new(
			std::io::ErrorKind::NotADirectory,
			"not a directory",
		));
	}
	fs4::available_space(dir)
}
