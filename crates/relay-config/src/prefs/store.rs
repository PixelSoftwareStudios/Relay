//! Single-writer preference store with backup recovery.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::migrate::migrate;
use super::model::Preferences;
use crate::error::{ConfigError, Result};
use crate::fsx::{WriterLock, backup_path, corrupt_path, read_if_exists, write_atomic};
use crate::notice::Notice;

/// Loaded preferences plus anything the user must be told about.
#[derive(Debug)]
pub struct Loaded {
	pub preferences: Preferences,
	pub notices: Vec<Notice>,
}

/// Parses, migrates and validates preference bytes.
fn parse(path: &Path, bytes: &[u8]) -> Result<(Preferences, Option<u32>)> {
	let parse_err = |m: String| ConfigError::Parse {
		path: path.to_path_buf(),
		message: m,
	};
	let text = std::str::from_utf8(bytes).map_err(|e| parse_err(e.to_string()))?;
	let mut table: toml::Table = text
		.parse()
		.map_err(|e: toml::de::Error| parse_err(e.to_string()))?;
	let from = migrate(&mut table)?;
	let prefs: Preferences = table
		.try_into()
		.map_err(|e: toml::de::Error| parse_err(e.to_string()))?;
	prefs.validate()?;
	let migrated = (from != prefs.config_version).then_some(from);
	Ok((prefs, migrated))
}

/// Owns the preferences file; holding the store holds the writer lock.
#[derive(Debug)]
pub struct PreferenceStore {
	path: PathBuf,
	_lock: Arc<WriterLock>,
}

impl PreferenceStore {
	/// Binds to `path`, sharing the process-wide writer lock.
	pub fn open(path: impl Into<PathBuf>, lock: Arc<WriterLock>) -> Result<Self> {
		Ok(Self {
			path: path.into(),
			_lock: lock,
		})
	}

	/// Loads preferences, recovering from damage with explicit notices.
	///
	/// Recovered or reset preferences never carry a microphone selection and always
	/// start calls receive-only, so a crash cannot silently open a microphone.
	pub fn load(&self) -> Result<Loaded> {
		let mut notices = Vec::new();
		let main = read_if_exists(&self.path)?;
		let Some(bytes) = main else {
			// A missing main file next to a backup means a crash mid-replace.
			return self.load_backup_or_default(None, notices);
		};
		match parse(&self.path, &bytes) {
			Ok((prefs, migrated_from)) => {
				if let Some(from) = migrated_from {
					notices.push(Notice::PreferencesMigrated {
						from,
						to: prefs.config_version,
					});
				}
				Ok(Loaded {
					preferences: prefs,
					notices,
				})
			}
			Err(e @ ConfigError::UnsupportedVersion { .. }) => Err(e),
			Err(_) => {
				let preserved = corrupt_path(&self.path);
				let kept = std::fs::copy(&self.path, &preserved)
					.is_ok()
					.then_some(preserved);
				self.load_backup_or_default(kept, notices)
			}
		}
	}

	fn load_backup_or_default(
		&self,
		preserved: Option<PathBuf>,
		mut notices: Vec<Notice>,
	) -> Result<Loaded> {
		let bak = backup_path(&self.path);
		let backup = read_if_exists(&bak)?;
		if let Some(bytes) = &backup
			&& let Ok((mut prefs, _)) = parse(&bak, bytes)
		{
			prefs.clear_microphone();
			notices.push(Notice::PreferencesRecoveredFromBackup);
			return Ok(Loaded {
				preferences: prefs,
				notices,
			});
		}
		if preserved.is_some() || backup.is_some() {
			notices.push(Notice::PreferencesReset { preserved });
		}
		Ok(Loaded {
			preferences: Preferences::default(),
			notices,
		})
	}

	/// Validates and atomically saves `prefs`.
	pub fn save(&self, prefs: &Preferences) -> Result<()> {
		prefs.validate()?;
		let text = toml::to_string_pretty(prefs).map_err(|e| ConfigError::Serialize {
			what: "preferences",
			message: e.to_string(),
		})?;
		write_atomic(&self.path, text.as_bytes(), false)
	}
}
