//! Human-editable TOML export and import of settings, for support and backups.
//!
//! The format holds preferences only. Keys, tokens and trust entries are never
//! part of it; the identity key lives in the OS credential store.

use std::collections::BTreeMap;

use toml::Table;

use super::flat::{flatten, leaves};
use super::migrate::migrate;
use super::model::Preferences;
use crate::error::{ConfigError, Result};

const HEADER: &str = "# Relay settings export. Preferences only: no keys, tokens or trust data.\n";

/// Serializes `prefs` as TOML.
pub fn export_toml(prefs: &Preferences) -> Result<String> {
	let text = toml::to_string_pretty(prefs).map_err(|e| ConfigError::Serialize {
		what: "preferences",
		message: e.to_string(),
	})?;
	Ok(format!("{HEADER}{text}"))
}

/// Parses, migrates and validates exported TOML. Unknown settings are an error so
/// that typos are not silently ignored.
pub fn import_toml(text: &str) -> Result<Preferences> {
	let (prefs, _) = parse_toml(text, true)?;
	Ok(prefs)
}

/// Like [`import_toml`] (`strict` rejects unknown settings), also returning the schema version the text came from when
/// it was older than the current one.
pub(crate) fn parse_toml(text: &str, strict: bool) -> Result<(Preferences, Option<u32>)> {
	let parse_err = |m: String| ConfigError::Parse {
		path: "<settings>".into(),
		message: m,
	};
	let mut table: Table = text
		.parse()
		.map_err(|e: toml::de::Error| parse_err(e.to_string()))?;
	let from = migrate(&mut table)?;
	if strict {
		let mut given = BTreeMap::new();
		leaves("", &table, &mut given);
		let known = flatten(&Preferences::default())?;
		if let Some(unknown) = given.keys().find(|k| !known.contains_key(*k)) {
			return Err(ConfigError::InvalidPreference(format!(
				"unknown setting `{unknown}`"
			)));
		}
	}
	let prefs: Preferences = table
		.try_into()
		.map_err(|e: toml::de::Error| parse_err(e.to_string()))?;
	prefs.validate()?;
	let migrated = (from != prefs.config_version).then_some(from);
	Ok((prefs, migrated))
}
