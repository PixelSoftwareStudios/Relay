//! Crash-safe file replacement.
//!
//! Sequence: write `<file>.tmp`, fsync it, optionally copy the current file to
//! `<file>.bak`, rename the temp file over the target, fsync the directory. A crash
//! at any step leaves either the old file, the backup, or the new file readable,
//! never a partial one. The temp name is fixed, so callers serialize writers with
//! a [`crate::lock::ProcessLock`]; a stale temp file is removed and recreated
//! exclusively, so a planted symlink is never followed.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::names::{backup_path, with_suffix};
use super::private::{create_private_new, ensure_private_dir};
use crate::error::{IoError, Result};

/// Options for an atomic replacement.
#[derive(Debug, Clone)]
pub struct AtomicWrite {
	path: PathBuf,
	private: bool,
	backup: bool,
}

impl AtomicWrite {
	/// Replace `path` (created with default permissions, no backup).
	pub fn new(path: impl Into<PathBuf>) -> Self {
		Self {
			path: path.into(),
			private: false,
			backup: false,
		}
	}

	/// Create the file owner-only (0600 on Unix).
	#[must_use]
	pub fn private(mut self, private: bool) -> Self {
		self.private = private;
		self
	}

	/// Keep the previous content as `<file>.bak`.
	#[must_use]
	pub fn backup(mut self, backup: bool) -> Self {
		self.backup = backup;
		self
	}

	/// Writes and fsyncs the temp file without touching the target yet.
	pub fn stage(&self, bytes: &[u8]) -> Result<Staged> {
		let dir = self.path.parent().unwrap_or_else(|| Path::new("."));
		if self.private {
			ensure_private_dir(dir)?;
		} else {
			fs::create_dir_all(dir).map_err(|e| IoError::io(dir, e))?;
		}
		let tmp = with_suffix(&self.path, ".tmp");
		match fs::remove_file(&tmp) {
			Ok(()) => {}
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
			Err(e) => return Err(IoError::io(&tmp, e)),
		}
		let mut file = if self.private {
			create_private_new(&tmp)?
		} else {
			fs::OpenOptions::new()
				.write(true)
				.create_new(true)
				.open(&tmp)
				.map_err(|e| IoError::io(&tmp, e))?
		};
		file.write_all(bytes).map_err(|e| IoError::io(&tmp, e))?;
		file.sync_all().map_err(|e| IoError::io(&tmp, e))?;
		Ok(Staged {
			tmp,
			target: self.path.clone(),
			backup: self.backup,
		})
	}

	/// Atomically replaces the target with `bytes`.
	pub fn write(&self, bytes: &[u8]) -> Result<()> {
		self.stage(bytes)?.commit()
	}
}

/// A durable temp file waiting to be renamed over its target.
#[derive(Debug)]
pub struct Staged {
	tmp: PathBuf,
	target: PathBuf,
	backup: bool,
}

impl Staged {
	/// The temp file path.
	pub fn temp_path(&self) -> &Path {
		&self.tmp
	}

	/// Makes the staged content the target's content.
	pub fn commit(self) -> Result<()> {
		let path = &self.target;
		if self.backup && path.exists() {
			let bak = backup_path(path);
			fs::copy(path, &bak).map_err(|e| IoError::io(&bak, e))?;
			File::open(&bak)
				.and_then(|f| f.sync_all())
				.map_err(|e| IoError::io(&bak, e))?;
		}
		fs::rename(&self.tmp, path).map_err(|e| IoError::io(path, e))?;
		if let Some(dir) = path.parent() {
			sync_dir(dir);
		}
		Ok(())
	}
}

/// Best-effort directory fsync so a rename itself is durable.
pub fn sync_dir(dir: &Path) {
	#[cfg(unix)]
	if let Ok(handle) = File::open(if dir.as_os_str().is_empty() {
		Path::new(".")
	} else {
		dir
	}) {
		let _ = handle.sync_all();
	}
	#[cfg(not(unix))]
	let _ = dir;
}
