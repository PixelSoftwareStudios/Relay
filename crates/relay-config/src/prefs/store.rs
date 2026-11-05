//! Preference store over the SQLite `prefs` table.

use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use super::flat;
use super::model::Preferences;
use super::portable;
use super::sql;
use crate::db::{Database, SCHEMA_VERSION};
use crate::error::{ConfigError, Result};
use crate::notice::Notice;
use crate::paths::Paths;

/// Loaded preferences plus anything the user must be told about.
#[derive(Debug)]
pub struct Loaded {
	pub preferences: Preferences,
	pub notices: Vec<Notice>,
}

/// Reads and writes preferences. Holding the database holds the writer lock.
#[derive(Debug, Clone)]
pub struct PreferenceStore {
	db: Database,
}

impl PreferenceStore {
	pub fn open(db: &Database) -> Result<Self> {
		Ok(Self { db: db.clone() })
	}

	/// Reads preferences without the writer lock, recovery or any write, for tools
	/// that must not disturb a running daemon (`doctor`, `config get`). Returns
	/// `None` when no database exists yet or nothing was ever saved.
	pub fn read_only(paths: &Paths) -> Result<Option<Preferences>> {
		let path = paths.database_file();
		if !path.exists() {
			return Ok(None);
		}
		let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
		conn.busy_timeout(Duration::from_secs(2))?;
		let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
		if version > i64::from(SCHEMA_VERSION) {
			return Err(ConfigError::UnsupportedVersion {
				found: version,
				supported: SCHEMA_VERSION,
			});
		}
		if version == 0 {
			return Ok(None);
		}
		sql::load(&conn)
	}

	/// Loads preferences. Recovery from a damaged database is reported by
	/// [`Database::open`]; rows that fail validation (for example after manual
	/// tampering) yield safe defaults, no microphone, and a notice.
	pub fn load(&self) -> Result<Loaded> {
		match self.db.with(|c| sql::load(c)) {
			Ok(found) => Ok(Loaded {
				preferences: found.unwrap_or_default(),
				notices: Vec::new(),
			}),
			Err(e @ (ConfigError::UnsupportedVersion { .. } | ConfigError::Database(_))) => Err(e),
			Err(_) => Ok(Loaded {
				preferences: Preferences::default(),
				notices: vec![Notice::PreferencesReset { preserved: None }],
			}),
		}
	}

	/// Validates and saves `prefs` in one transaction.
	pub fn save(&self, prefs: &Preferences) -> Result<()> {
		prefs.validate()?;
		self.db.with(|c| sql::save(c, prefs))?;
		self.db.refresh_backup(false);
		Ok(())
	}

	/// One setting as text (see [`flat::get`]).
	pub fn get(&self, key: &str) -> Result<String> {
		flat::get(&self.load()?.preferences, key)
	}

	/// Sets one setting from user text, validates and saves; returns the result.
	pub fn set(&self, key: &str, raw: &str) -> Result<Preferences> {
		let next = flat::set(&self.load()?.preferences, key, raw)?;
		self.save(&next)?;
		Ok(next)
	}

	/// Current settings as human-editable TOML (no secrets).
	pub fn export_toml(&self) -> Result<String> {
		portable::export_toml(&self.load()?.preferences)
	}

	/// Replaces all settings from exported TOML after validating it.
	pub fn import_toml(&self, text: &str) -> Result<Preferences> {
		let prefs = portable::import_toml(text)?;
		self.save(&prefs)?;
		Ok(prefs)
	}
}
