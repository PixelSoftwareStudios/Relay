//! Temporary files created exclusively next to their destination.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::entropy::OsEntropy;
use crate::error::{IoError, Result};

/// A temp file with an unpredictable name, created exclusively (so an existing
/// file or planted symlink is never reused) with mode 0600. It is deleted on
/// drop unless [`TempFile::keep`] was called.
#[derive(Debug)]
pub struct TempFile {
	file: Option<File>,
	path: PathBuf,
	keep: bool,
}

impl TempFile {
	/// Creates `.<prefix><random>.part` inside `dir`.
	pub fn create_in(dir: &Path, prefix: &str) -> Result<Self> {
		for _ in 0..16 {
			let suffix = OsEntropy.next_u128();
			let path = dir.join(format!(".{prefix}{suffix:032x}.part"));
			let mut options = OpenOptions::new();
			options.write(true).create_new(true);
			#[cfg(unix)]
			{
				use std::os::unix::fs::OpenOptionsExt;
				options.mode(0o600);
			}
			match options.open(&path) {
				Ok(file) => {
					return Ok(Self {
						file: Some(file),
						path,
						keep: false,
					});
				}
				Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
				Err(e) => return Err(IoError::io(path, e)),
			}
		}
		Err(IoError::io(
			dir,
			std::io::Error::new(
				std::io::ErrorKind::AlreadyExists,
				"no unused temporary name",
			),
		))
	}

	/// The temp file path.
	pub fn path(&self) -> &Path {
		&self.path
	}

	/// The open file, until [`TempFile::close`].
	pub fn file(&mut self) -> Option<&mut File> {
		self.file.as_mut()
	}

	/// Closes the handle (needed before renaming on Windows).
	pub fn close(&mut self) {
		self.file = None;
	}

	/// Leaves the file on disk when dropped (after it was renamed or must stay).
	pub fn keep(&mut self) {
		self.keep = true;
	}
}

impl Drop for TempFile {
	fn drop(&mut self) {
		self.file = None;
		if !self.keep {
			let _ = fs::remove_file(&self.path);
		}
	}
}
