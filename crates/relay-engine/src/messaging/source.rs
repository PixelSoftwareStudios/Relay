//! The sender's file: validation, hashing over a snapshot, change detection, chunk reads
//! and the optional text preview. Every function here blocks and must run on a blocking
//! worker. The daemon opens the file itself with the user's own filesystem access; the
//! client only names a path.

use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use relay_messages::limits::{MAX_FILE_BYTES, MAX_PREVIEW_LINES};
use thiserror::Error;

/// Hash buffer.
const HASH_BUFFER: usize = 1024 * 1024;
/// Preview bytes read from the file (the wire carries at most 15 KiB).
const PREVIEW_READ_BYTES: usize = 15 * 1024;

/// Why a path cannot be offered.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SourceError {
	/// Not an absolute path without `..`.
	#[error("the path must be absolute and free of `..`")]
	NotAbsolute,
	/// The final component is a symbolic link.
	#[error("symbolic links are not offered")]
	Symlink,
	/// Not a regular file.
	#[error("not a regular file")]
	NotRegular,
	/// Over the configured or protocol size limit.
	#[error("file is larger than the offer limit")]
	TooLarge,
	/// The file changed while it was read.
	#[error("the file changed while it was read")]
	Changed,
	/// The file could not be opened or read.
	#[error("the file could not be read")]
	Io,
	/// No usable text preview.
	#[error("the file has no text preview")]
	NoPreview,
}

/// What identifies the file as it was hashed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
	len: u64,
	mtime: Option<SystemTime>,
	node: Option<(u64, u64)>,
}

impl Facts {
	fn of(meta: &Metadata) -> Self {
		#[cfg(unix)]
		let node = {
			use std::os::unix::fs::MetadataExt;
			Some((meta.dev(), meta.ino()))
		};
		#[cfg(not(unix))]
		let node = None;
		Self {
			len: meta.len(),
			mtime: meta.modified().ok(),
			node,
		}
	}
}

/// A hashed, ready-to-offer file.
#[derive(Debug, Clone)]
pub struct Prepared {
	/// The path as given.
	pub path: PathBuf,
	/// Last component, for the offer (the receiver sanitizes again).
	pub name: String,
	/// Bytes.
	pub size: u64,
	/// BLAKE3 over the snapshot that was read.
	pub hash: [u8; 32],
	/// Identity for change detection.
	pub facts: Facts,
}

fn check_path(path: &Path) -> Result<(), SourceError> {
	if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
		return Err(SourceError::NotAbsolute);
	}
	Ok(())
}

/// Opens `path` as a regular, non-symlink file and returns its facts. The opened handle is
/// compared with the path afterwards so a swap between the check and the open is noticed.
pub fn open_checked(path: &Path) -> Result<(File, Facts), SourceError> {
	check_path(path)?;
	let linked = std::fs::symlink_metadata(path).map_err(|_| SourceError::Io)?;
	if linked.file_type().is_symlink() {
		return Err(SourceError::Symlink);
	}
	if !linked.is_file() {
		return Err(SourceError::NotRegular);
	}
	let file = File::open(path).map_err(|_| SourceError::Io)?;
	let meta = file.metadata().map_err(|_| SourceError::Io)?;
	if !meta.is_file() {
		return Err(SourceError::NotRegular);
	}
	let (opened, linked) = (Facts::of(&meta), Facts::of(&linked));
	if opened.node != linked.node {
		return Err(SourceError::Changed);
	}
	Ok((file, opened))
}

/// Validates, then hashes the file while reading it (the snapshot), and checks that it did
/// not change meanwhile.
pub fn prepare(path: &str, max_bytes: u64) -> Result<Prepared, SourceError> {
	let path = PathBuf::from(path);
	let (mut file, facts) = open_checked(&path)?;
	if facts.len > max_bytes.min(MAX_FILE_BYTES) {
		return Err(SourceError::TooLarge);
	}
	let mut hasher = blake3::Hasher::new();
	let mut buffer = vec![0u8; HASH_BUFFER];
	let mut total = 0u64;
	loop {
		let n = file.read(&mut buffer).map_err(|_| SourceError::Io)?;
		if n == 0 {
			break;
		}
		total += n as u64;
		if total > facts.len {
			return Err(SourceError::Changed);
		}
		hasher.update(&buffer[..n]);
	}
	if total != facts.len || !unchanged(&path, &file, &facts) {
		return Err(SourceError::Changed);
	}
	let name = path
		.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_default();
	Ok(Prepared {
		path,
		name,
		size: facts.len,
		hash: hasher.finalize().into(),
		facts,
	})
}

/// True when the handle and the path still describe the hashed file.
pub fn unchanged(path: &Path, file: &File, expected: &Facts) -> bool {
	let Ok(now) = file.metadata() else {
		return false;
	};
	let Ok(linked) = std::fs::symlink_metadata(path) else {
		return false;
	};
	!linked.file_type().is_symlink()
		&& Facts::of(&now) == *expected
		&& Facts::of(&linked) == *expected
}

/// The open source of a running transfer.
#[derive(Clone)]
pub struct Source {
	file: Arc<Mutex<File>>,
	path: PathBuf,
	facts: Facts,
}

impl Source {
	/// Opens the file again for the transfer and checks it is still the hashed one.
	pub fn open(path: &Path, facts: &Facts) -> Result<Self, SourceError> {
		let (file, now) = open_checked(path)?;
		if now != *facts {
			return Err(SourceError::Changed);
		}
		Ok(Self {
			file: Arc::new(Mutex::new(file)),
			path: path.to_owned(),
			facts: facts.clone(),
		})
	}

	/// Whether the file is still the one that was hashed.
	pub fn unchanged(&self) -> bool {
		let file = self.file.lock().unwrap_or_else(|p| p.into_inner());
		unchanged(&self.path, &file, &self.facts)
	}

	/// Reads exactly `len` bytes at `offset`.
	pub fn read(&self, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
		let mut file = self.file.lock().unwrap_or_else(|p| p.into_inner());
		file.seek(SeekFrom::Start(offset))?;
		let mut data = vec![0u8; len];
		file.read_exact(&mut data)?;
		Ok(data)
	}
}

/// The first lines of a text file for an approved preview: valid UTF-8, at most 15 KiB and
/// 100 lines, passing text sanitization.
pub fn preview_text(path: &Path) -> Result<String, SourceError> {
	let (mut file, _) = open_checked(path)?;
	let mut raw = Vec::new();
	file.by_ref()
		.take(PREVIEW_READ_BYTES as u64)
		.read_to_end(&mut raw)
		.map_err(|_| SourceError::Io)?;
	let text = match std::str::from_utf8(&raw) {
		Ok(text) => text,
		// A multi-byte character cut by the read limit is fine; anything else is binary.
		Err(error) if error.error_len().is_none() => {
			std::str::from_utf8(&raw[..error.valid_up_to()]).map_err(|_| SourceError::NoPreview)?
		}
		Err(_) => return Err(SourceError::NoPreview),
	};
	let head: String = text.split_inclusive('\n').take(MAX_PREVIEW_LINES).collect();
	let head = head.trim_end_matches(['\n', '\r']);
	if head.is_empty() {
		return Err(SourceError::NoPreview);
	}
	relay_messages::transfer::validate_preview(head).map_err(|_| SourceError::NoPreview)
}
