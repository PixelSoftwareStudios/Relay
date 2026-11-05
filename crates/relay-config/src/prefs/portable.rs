//! Human-editable TOML export and import of settings, for support and backups.
//!
//! The format holds preferences only. Keys, tokens and trust entries are never
//! part of it; the identity key lives in the OS credential store.

use std::collections::BTreeMap;

use toml::Table;

use super::flat::{VERSION_KEY, flatten, leaves};
use super::model::{CURRENT_VERSION, Preferences};
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

/// Parses and validates exported TOML. Unknown settings are an error so that typos
/// are not silently ignored, and a newer `config_version` is refused.
pub fn import_toml(text: &str) -> Result<Preferences> {
	let parse_err = |m: String| ConfigError::Parse {
		path: "<settings>".into(),
		message: m,
	};
	let table: Table = text
		.parse()
		.map_err(|e: toml::de::Error| parse_err(e.to_string()))?;
	if let Some(found) = table.get(VERSION_KEY) {
		match found.as_integer() {
			Some(v) if v > i64::from(CURRENT_VERSION) => {
				return Err(ConfigError::UnsupportedVersion {
					found: v,
					supported: CURRENT_VERSION,
				});
			}
			Some(v) if v >= 0 => {}
			_ => return Err(ConfigError::BadVersion),
		}
	}
	let mut given = BTreeMap::new();
	leaves("", &table, &mut given);
	let known = flatten(&Preferences::default())?;
	if let Some(unknown) = given.keys().find(|k| !known.contains_key(*k)) {
		return Err(ConfigError::InvalidPreference(format!(
			"unknown setting `{unknown}`"
		)));
	}
	let prefs: Preferences = table
		.try_into()
		.map_err(|e: toml::de::Error| parse_err(e.to_string()))?;
	prefs.validate()?;
	Ok(prefs)
}
