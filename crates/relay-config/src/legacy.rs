//! One-time import of the pre-SQLite `config.toml` and `trust.toml`.
//!
//! The same safety rules as before apply: damaged files fall back to their backup
//! (microphone cleared, receive-only) or to defaults, always with a [`Notice`]; a
//! preferences file from a newer build aborts the import. After a successful import
//! the files are renamed to `*.imported` and never written again.

use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::db::Database;
use crate::error::{ConfigError, Result};
use crate::fsx::{backup_path, corrupt_path, read_if_exists, with_suffix};
use crate::identity::TrustEntry;
use crate::identity::{insert_trust_entry, trust_count};
use crate::notice::Notice;
use crate::paths::Paths;
use crate::prefs::{Preferences, parse_toml, sql as prefs_sql};

const DONE_KEY: &str = "legacy_import";
const TRUST_VERSION: u32 = 1;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TrustFile {
	version: u32,
	entries: Vec<TrustEntry>,
}

fn parse_prefs(bytes: &[u8]) -> Result<(Preferences, Option<u32>)> {
	let text = std::str::from_utf8(bytes).map_err(|e| ConfigError::Parse {
		path: "config.toml".into(),
		message: e.to_string(),
	})?;
	parse_toml(text, false)
}

fn parse_trust(bytes: &[u8]) -> Option<TrustFile> {
	let file: TrustFile = toml::from_str(std::str::from_utf8(bytes).ok()?).ok()?;
	(1..=TRUST_VERSION).contains(&file.version).then_some(file)
}

fn keep_copy(path: &Path) -> Option<std::path::PathBuf> {
	let kept = corrupt_path(path);
	fs::copy(path, &kept).is_ok().then_some(kept)
}

/// Reads the legacy preferences, if any, with recovery notices.
fn legacy_prefs(path: &Path) -> Result<Option<(Preferences, Vec<Notice>)>> {
	let bak = backup_path(path);
	let main = read_if_exists(path)?;
	let mut preserved = None;
	if let Some(bytes) = &main {
		match parse_prefs(bytes) {
			Ok((prefs, from)) => {
				let notices = from
					.map(|from| Notice::PreferencesMigrated {
						from,
						to: prefs.config_version,
					})
					.into_iter()
					.collect();
				return Ok(Some((prefs, notices)));
			}
			Err(e @ ConfigError::UnsupportedVersion { .. }) => return Err(e),
			Err(_) => preserved = keep_copy(path),
		}
	}
	let backup = read_if_exists(&bak)?;
	if let Some(bytes) = &backup
		&& let Ok((mut prefs, _)) = parse_prefs(bytes)
	{
		prefs.clear_microphone();
		return Ok(Some((prefs, vec![Notice::PreferencesRecoveredFromBackup])));
	}
	if main.is_none() && backup.is_none() {
		return Ok(None);
	}
	Ok(Some((
		Preferences::default(),
		vec![Notice::PreferencesReset { preserved }],
	)))
}

/// Reads the legacy trust file, if any, with recovery notices.
fn legacy_trust(path: &Path) -> Result<Option<(Vec<TrustEntry>, Vec<Notice>)>> {
	let bak = backup_path(path);
	let main = read_if_exists(path)?;
	let backup = read_if_exists(&bak)?;
	let mut preserved = None;
	if let Some(bytes) = &main {
		if let Some(file) = parse_trust(bytes) {
			return Ok(Some((file.entries, Vec::new())));
		}
		preserved = keep_copy(path);
	}
	if main.is_none() && backup.is_none() {
		return Ok(None);
	}
	if let Some(file) = backup.as_deref().and_then(parse_trust) {
		return Ok(Some((file.entries, vec![Notice::TrustRecoveredFromBackup])));
	}
	Ok(Some((
		Vec::new(),
		vec![Notice::TrustStoreLost { preserved }],
	)))
}

fn valid(entry: &TrustEntry) -> bool {
	entry.public_key.len() == 64 && entry.public_key.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Imports legacy files once. Existing database content is never overwritten.
pub(crate) fn import(db: &Database, paths: &Paths) -> Result<Vec<Notice>> {
	let done = db.with(|c| {
		Ok(
			c.query_row("SELECT 1 FROM state WHERE key = ?1", [DONE_KEY], |_| Ok(()))
				.is_ok(),
		)
	})?;
	if done {
		return Ok(Vec::new());
	}
	let prefs_path = paths.legacy_preferences_file();
	let trust_path = paths.legacy_trust_file();
	let prefs = legacy_prefs(&prefs_path)?;
	let trust = legacy_trust(&trust_path)?;
	let mut notices = Vec::new();
	db.with(|conn| {
		let tx = conn.transaction()?;
		if let Some((prefs, found)) = prefs
			&& prefs_sql::load(&tx)?.is_none()
		{
			prefs_sql::save_in(&tx, &prefs)?;
			notices.extend(found);
		}
		if let Some((entries, found)) = trust
			&& trust_count(&tx)? == 0
		{
			for entry in entries.iter().filter(|e| valid(e)) {
				insert_trust_entry(&tx, entry)?;
			}
			notices.extend(found);
		}
		tx.execute(
			"INSERT OR REPLACE INTO state (key, value) VALUES (?1, x'01')",
			[DONE_KEY],
		)?;
		tx.commit()?;
		Ok(())
	})?;
	for path in [&prefs_path, &trust_path] {
		for source in [path.clone(), backup_path(path)] {
			if source.exists() {
				let _ = fs::rename(&source, with_suffix(&source, ".imported"));
			}
		}
	}
	Ok(notices)
}
