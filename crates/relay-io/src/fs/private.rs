//! Owner-only permissions. Each OS user gets private directories and files, so a
//! second user of the same PC can neither read nor modify them.
//!
//! On Unix directories are 0700 and files 0600; anything looser is tightened, and
//! if that is refused (the path belongs to someone else) the call fails. On
//! Windows the per-user profile ACLs isolate the directories; an explicit
//! owner-only DACL is not applied (see `docs/portability.md`).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::error::{IoError, Result};

/// The effective uid of this process.
#[cfg(unix)]
pub fn current_uid() -> u32 {
	rustix::process::geteuid().as_raw()
}

/// Creates `dir` (and missing parents) and makes sure group and others have no access.
pub fn ensure_private_dir(dir: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
		fs::DirBuilder::new()
			.recursive(true)
			.mode(0o700)
			.create(dir)
			.map_err(|e| IoError::io(dir, e))?;
		let meta = fs::metadata(dir).map_err(|e| IoError::io(dir, e))?;
		if meta.permissions().mode() & 0o077 != 0 {
			fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
				.map_err(|e| IoError::io(dir, e))?;
		}
	}
	#[cfg(not(unix))]
	fs::create_dir_all(dir).map_err(|e| IoError::io(dir, e))?;
	Ok(())
}

/// Creates (0700) or verifies a directory that holds an endpoint. Unlike
/// [`ensure_private_dir`] it refuses a symlink, a non-directory and a directory
/// owned by another user (it never follows or adopts what it did not create).
/// A loose mode on a directory we own is tightened.
pub fn ensure_owned_private_dir(dir: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
		match fs::symlink_metadata(dir) {
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
				fs::DirBuilder::new()
					.recursive(true)
					.mode(0o700)
					.create(dir)
					.map_err(|e| IoError::io(dir, e))?;
			}
			Err(e) => return Err(IoError::io(dir, e)),
			Ok(_) => {}
		}
		let meta = fs::symlink_metadata(dir).map_err(|e| IoError::io(dir, e))?;
		let insecure = |reason| IoError::Insecure {
			path: dir.to_path_buf(),
			reason,
		};
		if !meta.is_dir() {
			return Err(insecure("not a directory (symlinks are refused)"));
		}
		if meta.uid() != current_uid() {
			return Err(insecure("owned by another user"));
		}
		if meta.mode() & 0o077 != 0 {
			fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
				.map_err(|e| IoError::io(dir, e))?;
		}
		Ok(())
	}
	#[cfg(not(unix))]
	{
		fs::create_dir_all(dir).map_err(|e| IoError::io(dir, e))?;
		let meta = fs::symlink_metadata(dir).map_err(|e| IoError::io(dir, e))?;
		if !meta.is_dir() {
			return Err(IoError::Insecure {
				path: dir.to_path_buf(),
				reason: "not a directory (symlinks are refused)",
			});
		}
		Ok(())
	}
}

/// Restricts an existing file to its owner (mode 0600 on Unix).
pub fn restrict_file(path: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		let meta = fs::metadata(path).map_err(|e| IoError::io(path, e))?;
		if meta.permissions().mode() & 0o177 != 0 {
			fs::set_permissions(path, fs::Permissions::from_mode(0o600))
				.map_err(|e| IoError::io(path, e))?;
		}
	}
	#[cfg(not(unix))]
	let _ = path;
	Ok(())
}

/// Fails if `path` is group/world accessible on Unix (always passes elsewhere).
pub fn check_private_file(path: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		let meta = fs::metadata(path).map_err(|e| IoError::io(path, e))?;
		if meta.permissions().mode() & 0o077 != 0 {
			return Err(IoError::Insecure {
				path: path.to_path_buf(),
				reason: "file is group/world accessible",
			});
		}
	}
	#[cfg(not(unix))]
	let _ = path;
	Ok(())
}

fn private_options(options: &mut OpenOptions) -> &mut OpenOptions {
	#[cfg(unix)]
	{
		use std::os::unix::fs::OpenOptionsExt;
		options.mode(0o600);
	}
	options
}

/// Opens `path` for writing, creating it 0600 if missing, without truncating.
pub fn create_private_file(path: &Path) -> Result<File> {
	private_options(OpenOptions::new().write(true).create(true).truncate(false))
		.open(path)
		.map_err(|e| IoError::io(path, e))
}

/// Opens `path` for appending, creating it 0600. A symlink at `path` is refused
/// (`O_NOFOLLOW`), a file owned by someone else is refused and a looser mode on
/// our own file is tightened to 0600.
pub fn open_private_append(path: &Path) -> Result<File> {
	let mut options = OpenOptions::new();
	private_options(options.append(true).create(true));
	#[cfg(unix)]
	{
		use std::os::unix::fs::OpenOptionsExt;
		options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
	}
	let file = options.open(path).map_err(|e| IoError::io(path, e))?;
	#[cfg(unix)]
	{
		use std::os::unix::fs::{MetadataExt, PermissionsExt};
		let meta = file.metadata().map_err(|e| IoError::io(path, e))?;
		if meta.uid() != current_uid() {
			return Err(IoError::Insecure {
				path: path.to_path_buf(),
				reason: "owned by another user",
			});
		}
		if meta.permissions().mode() & 0o177 != 0 {
			file.set_permissions(fs::Permissions::from_mode(0o600))
				.map_err(|e| IoError::io(path, e))?;
		}
	}
	Ok(file)
}

/// Exclusively creates `path` (0600); fails if anything, even a symlink, exists.
pub fn create_private_new(path: &Path) -> Result<File> {
	private_options(OpenOptions::new().write(true).create_new(true))
		.open(path)
		.map_err(|e| IoError::io(path, e))
}

/// Writes `bytes` to `path`, truncating, creating it 0600 if missing.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
	let mut file = private_options(OpenOptions::new().write(true).create(true).truncate(true))
		.open(path)
		.map_err(|e| IoError::io(path, e))?;
	file.write_all(bytes).map_err(|e| IoError::io(path, e))
}

/// Replaces `path` with a fresh 0600 file holding `bytes`: the old file is
/// unlinked first so its previous mode, owner and links never carry over.
pub fn write_private_fresh(path: &Path, bytes: &[u8]) -> Result<()> {
	match fs::remove_file(path) {
		Ok(()) => {}
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
		Err(e) => return Err(IoError::io(path, e)),
	}
	let mut file = create_private_new(path)?;
	file.write_all(bytes).map_err(|e| IoError::io(path, e))
}
