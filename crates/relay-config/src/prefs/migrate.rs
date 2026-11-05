//! Schema migrations over the raw TOML table, applied one version at a time.

use toml::Table;
use toml::Value;

use super::model::CURRENT_VERSION;
use crate::error::{ConfigError, Result};

/// Reads the version of a raw table; a missing field means the legacy version 0.
fn version_of(table: &Table) -> Result<i64> {
	match table.get("config_version") {
		None => Ok(0),
		Some(Value::Integer(v)) if *v >= 0 => Ok(*v),
		Some(_) => Err(ConfigError::BadVersion),
	}
}

/// v0 -> v1: `[audio] input_device`/`output_device` became `preferred_*_id`.
/// A migrated microphone is kept as an explicit choice, never as system default.
fn v0_to_v1(table: &mut Table) {
	if let Some(Value::Table(audio)) = table.get_mut("audio") {
		for (old, new, sel) in [
			("input_device", "preferred_input_id", "input_selection"),
			("output_device", "preferred_output_id", "output_selection"),
		] {
			if let Some(value) = audio.remove(old) {
				audio.insert(new.to_owned(), value);
				audio.insert(sel.to_owned(), Value::String("explicit".to_owned()));
			}
		}
	}
}

/// Upgrades `table` to [`CURRENT_VERSION`]; returns the original version.
pub fn migrate(table: &mut Table) -> Result<u32> {
	let found = version_of(table)?;
	if found > i64::from(CURRENT_VERSION) {
		return Err(ConfigError::UnsupportedVersion {
			found,
			supported: CURRENT_VERSION,
		});
	}
	let from = found as u32;
	let mut version = from;
	while version < CURRENT_VERSION {
		match version {
			0 => v0_to_v1(table),
			_ => unreachable!("every version below CURRENT_VERSION has a migration"),
		}
		version += 1;
	}
	table.insert(
		"config_version".to_owned(),
		Value::Integer(i64::from(CURRENT_VERSION)),
	);
	Ok(from)
}
