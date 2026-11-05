//! Owner-only permissions. Each OS user gets private directories and files, so a
//! second user of the same PC can neither read nor modify them.
//!
//! On Unix directories are 0700 and files 0600; anything looser is tightened, and
//! if that is refused (the path belongs to someone else) the call fails. On Windows
//! the per-user profile ACLs already isolate the directories.

use std::fs;
use std::path::Path;

use crate::error::{ConfigError, Result};

/// Creates `dir` (and missing parents) and makes sure group and others have no access.
pub fn ensure_private_dir(dir: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
		fs::DirBuilder::new()
			.recursive(true)
			.mode(0o700)
			.create(dir)
			.map_err(|e| ConfigError::io(dir, e))?;
		let meta = fs::metadata(dir).map_err(|e| ConfigError::io(dir, e))?;
		if meta.permissions().mode() & 0o077 != 0 {
			fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
				.map_err(|e| ConfigError::io(dir, e))?;
		}
	}
	#[cfg(not(unix))]
	fs::create_dir_all(dir).map_err(|e| ConfigError::io(dir, e))?;
	Ok(())
}

/// Restricts an existing file to its owner (mode 0600 on Unix).
pub fn restrict_file(path: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		let meta = fs::metadata(path).map_err(|e| ConfigError::io(path, e))?;
		if meta.permissions().mode() & 0o177 != 0 {
			fs::set_permissions(path, fs::Permissions::from_mode(0o600))
				.map_err(|e| ConfigError::io(path, e))?;
		}
	}
	#[cfg(not(unix))]
	let _ = path;
	Ok(())
}
