//! Free space of the filesystem holding a directory.

use std::path::Path;

/// Bytes available to this user on the filesystem containing `dir`.
pub fn free_space(dir: &Path) -> std::io::Result<u64> {
	fs4::available_space(dir)
}
