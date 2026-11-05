//! Persistent trust store: which peer keys are TOFU-pinned or user-verified.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, Result};
use crate::fsx::{WriterLock, backup_path, corrupt_path, read_if_exists, write_atomic};
use crate::notice::Notice;

const TRUST_VERSION: u32 = 1;
const MAX_ENTRIES: usize = 4096;
const MAX_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
	/// Pinned on first use; not compared out of band.
	Tofu,
	/// The user compared the safety code out of band.
	Verified,
}

/// One pinned peer. Keys are lowercase hex of the 32-byte public key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustEntry {
	pub public_key: String,
	pub display_name: String,
	pub level: TrustLevel,
	/// Unix seconds when first pinned.
	pub first_seen: u64,
}

/// Outcome of presenting a peer key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustDecision {
	/// Previously unseen key; caller may pin it.
	Unknown,
	Known(TrustLevel),
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct TrustFile {
	version: u32,
	entries: Vec<TrustEntry>,
}

fn key_hex(key: &[u8; 32]) -> String {
	hex::encode(key)
}

/// Trust data bound to its file. Holds the shared writer lock.
#[derive(Debug)]
pub struct TrustStore {
	path: PathBuf,
	entries: Vec<TrustEntry>,
	_lock: Arc<WriterLock>,
}

fn parse(bytes: &[u8]) -> Option<TrustFile> {
	let file: TrustFile = toml::from_str(std::str::from_utf8(bytes).ok()?).ok()?;
	(file.version >= 1 && file.version <= TRUST_VERSION).then_some(file)
}

impl TrustStore {
	/// Opens the store. Damage is reported as notices, never hidden, and the
	/// damaged file is preserved next to the original.
	pub fn open(path: impl Into<PathBuf>, lock: Arc<WriterLock>) -> Result<(Self, Vec<Notice>)> {
		let path = path.into();
		let mut notices = Vec::new();
		let mut entries = Vec::new();
		let mut damaged = false;
		match read_if_exists(&path)? {
			Some(bytes) => match parse(&bytes) {
				Some(file) => entries = file.entries,
				None => damaged = true,
			},
			None => {
				// Missing main file with a backup means a crash mid-replace.
				damaged = read_if_exists(&backup_path(&path))?.is_some();
			}
		}
		if damaged {
			let preserved = corrupt_path(&path);
			let kept = std::fs::copy(&path, &preserved)
				.is_ok()
				.then_some(preserved);
			match read_if_exists(&backup_path(&path))?.and_then(|b| parse(&b)) {
				Some(file) => {
					entries = file.entries;
					notices.push(Notice::TrustRecoveredFromBackup);
				}
				None => notices.push(Notice::TrustStoreLost { preserved: kept }),
			}
		}
		Ok((
			Self {
				path,
				entries,
				_lock: lock,
			},
			notices,
		))
	}

	pub fn entries(&self) -> &[TrustEntry] {
		&self.entries
	}

	pub fn lookup(&self, key: &[u8; 32]) -> TrustDecision {
		let hex = key_hex(key);
		self.entries
			.iter()
			.find(|e| e.public_key == hex)
			.map_or(TrustDecision::Unknown, |e| TrustDecision::Known(e.level))
	}

	/// Pins a new key as TOFU; an existing entry is left untouched.
	pub fn pin_tofu(&mut self, key: &[u8; 32], display_name: &str, now: u64) -> Result<()> {
		if matches!(self.lookup(key), TrustDecision::Known(_)) {
			return Ok(());
		}
		if self.entries.len() >= MAX_ENTRIES {
			return Err(ConfigError::InvalidPreference(
				"trust store is full".to_owned(),
			));
		}
		let name: String = display_name
			.chars()
			.filter(|c| !c.is_control())
			.take(MAX_NAME_CHARS)
			.collect();
		self.entries.push(TrustEntry {
			public_key: key_hex(key),
			display_name: name,
			level: TrustLevel::Tofu,
			first_seen: now,
		});
		self.persist()
	}

	/// Marks an already pinned key as verified. Returns false if unknown.
	pub fn mark_verified(&mut self, key: &[u8; 32]) -> Result<bool> {
		let hex = key_hex(key);
		let Some(entry) = self.entries.iter_mut().find(|e| e.public_key == hex) else {
			return Ok(false);
		};
		entry.level = TrustLevel::Verified;
		self.persist()?;
		Ok(true)
	}

	/// Removes a peer. Returns whether an entry existed.
	pub fn remove(&mut self, key: &[u8; 32]) -> Result<bool> {
		let hex = key_hex(key);
		let before = self.entries.len();
		self.entries.retain(|e| e.public_key != hex);
		let removed = self.entries.len() != before;
		if removed {
			self.persist()?;
		}
		Ok(removed)
	}

	fn persist(&self) -> Result<()> {
		let file = TrustFile {
			version: TRUST_VERSION,
			entries: self.entries.clone(),
		};
		let text = toml::to_string_pretty(&file).map_err(|e| ConfigError::Serialize {
			what: "trust store",
			message: e.to_string(),
		})?;
		write_atomic(&self.path, text.as_bytes(), true)
	}
}
