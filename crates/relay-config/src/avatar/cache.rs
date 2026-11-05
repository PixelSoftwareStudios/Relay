//! Content-addressed avatar cache with a hard size bound and oldest-first eviction.

use std::fs::{self, File};
use std::path::PathBuf;
use std::time::SystemTime;

use super::limits::CACHE_MAX_BYTES;
use super::process::{Avatar, validate_canonical};
use crate::error::{ConfigError, Result};

/// Files are named `<blake3 hex>.png`; reads re-verify the hash.
#[derive(Debug)]
pub struct AvatarCache {
	dir: PathBuf,
	max_bytes: u64,
}

impl AvatarCache {
	pub fn open(dir: impl Into<PathBuf>) -> Result<Self> {
		Self::with_limit(dir, CACHE_MAX_BYTES)
	}

	pub fn with_limit(dir: impl Into<PathBuf>, max_bytes: u64) -> Result<Self> {
		let dir = dir.into();
		fs::create_dir_all(&dir).map_err(|e| ConfigError::io(&dir, e))?;
		Ok(Self { dir, max_bytes })
	}

	fn file(&self, hash: &[u8; 32]) -> PathBuf {
		self.dir.join(format!("{}.png", hex::encode(hash)))
	}

	/// Stores a locally processed avatar.
	pub fn put(&self, avatar: &Avatar) -> Result<()> {
		self.write(&avatar.hash, &avatar.png)
	}

	/// Validates and stores bytes received from a peer; returns their hash.
	pub fn put_untrusted(&self, bytes: &[u8]) -> Result<[u8; 32]> {
		let hash = validate_canonical(bytes)?;
		self.write(&hash, bytes)?;
		Ok(hash)
	}

	fn write(&self, hash: &[u8; 32], bytes: &[u8]) -> Result<()> {
		if bytes.len() as u64 > self.max_bytes {
			return Ok(());
		}
		let path = self.file(hash);
		if !path.exists() {
			crate::fsx::write_atomic(&path, bytes, false)?;
			let _ = fs::remove_file(crate::fsx::backup_path(&path));
		}
		self.evict(hash)
	}

	/// Returns cached bytes if present and still matching their hash.
	pub fn get(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>> {
		let path = self.file(hash);
		let Some(bytes) = crate::fsx::read_if_exists(&path)? else {
			return Ok(None);
		};
		if blake3::hash(&bytes).as_bytes() != hash {
			let _ = fs::remove_file(&path);
			return Ok(None);
		}
		// Mark as recently used for eviction ordering.
		if let Ok(f) = File::options().write(true).open(&path) {
			let _ = f.set_modified(SystemTime::now());
		}
		Ok(Some(bytes))
	}

	/// Total bytes of cached avatars.
	pub fn size(&self) -> Result<u64> {
		Ok(self.list()?.iter().map(|(_, len, _)| len).sum())
	}

	fn list(&self) -> Result<Vec<(PathBuf, u64, SystemTime)>> {
		let mut out = Vec::new();
		for entry in fs::read_dir(&self.dir).map_err(|e| ConfigError::io(&self.dir, e))? {
			let entry = entry.map_err(|e| ConfigError::io(&self.dir, e))?;
			let path = entry.path();
			if path.extension().and_then(|e| e.to_str()) != Some("png") {
				continue;
			}
			let meta = entry.metadata().map_err(|e| ConfigError::io(&path, e))?;
			out.push((
				path,
				meta.len(),
				meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
			));
		}
		Ok(out)
	}

	/// Deletes least recently used files until under the bound; never the entry just written.
	fn evict(&self, keep: &[u8; 32]) -> Result<()> {
		let keep = self.file(keep);
		let mut files = self.list()?;
		let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
		files.sort_by_key(|(_, _, modified)| *modified);
		for (path, len, _) in files {
			if total <= self.max_bytes {
				break;
			}
			if path == keep {
				continue;
			}
			if fs::remove_file(&path).is_ok() {
				total -= len;
			}
		}
		Ok(())
	}
}
