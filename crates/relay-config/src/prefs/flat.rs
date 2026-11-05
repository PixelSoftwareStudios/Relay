//! Preferences as flat `section.key` entries with TOML-encoded values.
//!
//! This is both the SQLite row format and the vocabulary of `get`/`set`: a value
//! keeps its TOML type (`1.5`, `true`, `"text"`), so rows stay typed and readable.

use std::collections::BTreeMap;

use toml::{Table, Value};

use super::model::Preferences;
use crate::error::{ConfigError, Result};

/// Key holding the schema version; not user-settable.
pub const VERSION_KEY: &str = "config_version";

fn invalid(message: impl Into<String>) -> ConfigError {
	ConfigError::InvalidPreference(message.into())
}

/// Collects the leaf values of `table` under dotted keys.
pub(super) fn leaves(prefix: &str, table: &Table, out: &mut BTreeMap<String, Value>) {
	for (name, value) in table {
		let key = if prefix.is_empty() {
			name.clone()
		} else {
			format!("{prefix}.{name}")
		};
		match value {
			Value::Table(inner) => leaves(&key, inner, out),
			leaf => {
				out.insert(key, leaf.clone());
			}
		}
	}
}

fn table_of(prefs: &Preferences) -> Result<Table> {
	Table::try_from(prefs).map_err(|e| ConfigError::Serialize {
		what: "preferences",
		message: e.to_string(),
	})
}

/// Every setting as `(dotted key, TOML-encoded value)`.
pub(super) fn flatten(prefs: &Preferences) -> Result<BTreeMap<String, String>> {
	let mut map = BTreeMap::new();
	leaves("", &table_of(prefs)?, &mut map);
	Ok(map.into_iter().map(|(k, v)| (k, v.to_string())).collect())
}

fn parse_value(text: &str) -> Result<Value> {
	let mut table: Table = format!("v = {text}")
		.parse()
		.map_err(|e: toml::de::Error| invalid(format!("not a valid value: {e}")))?;
	table.remove("v").ok_or_else(|| invalid("empty value"))
}

/// Rebuilds and validates preferences from flat entries; missing keys take defaults.
pub(super) fn unflatten(
	entries: impl IntoIterator<Item = (String, String)>,
) -> Result<Preferences> {
	let mut root = Table::new();
	for (key, text) in entries {
		let mut parts: Vec<&str> = key.split('.').collect();
		let leaf = parts.pop().unwrap_or_default();
		let mut node = &mut root;
		for part in parts {
			node = match node
				.entry(part)
				.or_insert_with(|| Value::Table(Table::new()))
			{
				Value::Table(t) => t,
				_ => return Err(invalid(format!("conflicting key `{key}`"))),
			};
		}
		node.insert(leaf.to_owned(), parse_value(&text)?);
	}
	let prefs: Preferences = root
		.try_into()
		.map_err(|e: toml::de::Error| invalid(e.to_string()))?;
	if prefs.config_version > super::model::CURRENT_VERSION {
		return Err(ConfigError::UnsupportedVersion {
			found: i64::from(prefs.config_version),
			supported: super::model::CURRENT_VERSION,
		});
	}
	prefs.validate()?;
	Ok(prefs)
}

/// All settable keys, sorted.
pub fn keys() -> Vec<String> {
	flatten(&Preferences::default())
		.map(|m| m.into_keys().filter(|k| k != VERSION_KEY).collect())
		.unwrap_or_default()
}

/// Reads one setting as its TOML-encoded value (strings come back unquoted).
pub fn get(prefs: &Preferences, key: &str) -> Result<String> {
	let value = flatten(prefs)?
		.remove(key)
		.ok_or_else(|| invalid(format!("unknown setting `{key}`")))?;
	Ok(match parse_value(&value)? {
		Value::String(s) => s,
		_ => value,
	})
}

/// Sets one setting from user text and validates the result.
///
/// Text for a string setting is taken literally; other types are parsed as TOML
/// (`true`, `1.5`, `"rnnoise"` or bare `rnnoise` for enums).
pub fn set(prefs: &Preferences, key: &str, raw: &str) -> Result<Preferences> {
	if key == VERSION_KEY {
		return Err(invalid("config_version cannot be set"));
	}
	let mut flat = flatten(prefs)?;
	let current = flat
		.get(key)
		.ok_or_else(|| invalid(format!("unknown setting `{key}`")))?;
	let literal = Value::String(raw.to_owned()).to_string();
	let is_text = matches!(parse_value(current)?, Value::String(_));
	let encoded = if is_text || parse_value(raw).is_err() {
		literal
	} else {
		raw.to_owned()
	};
	flat.insert(key.to_owned(), encoded);
	unflatten(flat)
}
