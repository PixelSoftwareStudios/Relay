//! `relay limits`: every storage and retention limit with its current value, default and range.
//!
//! The values come from the running daemon (`GetSettings`, which also reports what is in use
//! next to a limit) and otherwise from the settings database, read without a lock. Change one
//! with `relay config set limits.<name> <value>`.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};

use relay_config::Paths;
use relay_config::prefs::Limits;
use relay_config::prefs::limits::SPECS;
use serde::Serialize;

use super::table::{self, Layout};
use super::{Out, config};
use crate::cli::{CliError, GlobalArgs};
use crate::client::Manage;

const MIB: u64 = 1024 * 1024;

/// One limit.
#[derive(Debug, Serialize)]
pub struct LimitRow {
	pub key: &'static str,
	pub current: u32,
	pub default: u32,
	pub min: u32,
	pub max: u32,
	pub unit: &'static str,
	/// What is in use now, in bytes, when the running daemon knows it cheaply.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub used_bytes: Option<u64>,
}

/// All limits, in display order.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct LimitTable(pub Vec<LimitRow>);

impl LimitTable {
	/// Rows for `limits` and the usage a daemon reported.
	pub fn new(limits: &Limits, usage: &BTreeMap<String, u64>) -> Self {
		Self(
			SPECS
				.iter()
				.zip(limits.values())
				.map(|(spec, current)| LimitRow {
					key: spec.key,
					current,
					default: spec.default,
					min: spec.min,
					max: spec.max,
					unit: spec.unit,
					used_bytes: usage.get(spec.key).copied(),
				})
				.collect(),
		)
	}

	/// Text table.
	pub fn render(&self, layout: Layout) -> String {
		let mut t = table::with_header(layout, &["LIMIT", "CURRENT", "DEFAULT", "RANGE", "IN USE"]);
		for row in &self.0 {
			t.add_row([
				row.key.to_owned(),
				format!("{} {}", row.current, row.unit),
				format!("{} {}", row.default, row.unit),
				format!("{}..{} {}", row.min, row.max, row.unit),
				row.used_bytes.map_or_else(
					|| "-".to_owned(),
					|b| format!("{:.1} MiB", b as f64 / MIB as f64),
				),
			]);
		}
		table::render(t)
	}
}

impl Display for LimitTable {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.render(Layout::detect()))
	}
}

/// Print the table, reading the platform's settings directory.
pub async fn run(global: &GlobalArgs, out: &mut Out<'_>) -> Result<(), CliError> {
	let paths = Paths::platform().map_err(|e| CliError::Other(e.into()))?;
	run_in(global, &paths, out).await
}

/// Print the table, reading the settings under `paths`.
pub async fn run_in(global: &GlobalArgs, paths: &Paths, out: &mut Out<'_>) -> Result<(), CliError> {
	let (limits, usage) = match config::engine_running(global).await? {
		Some(mut engine) => read_live(&mut engine).await?,
		None => (config::load_prefs(paths)?.limits, BTreeMap::new()),
	};
	let table = LimitTable::new(&limits, &usage);
	out.emit_with(&table, || table.render(Layout::detect()))
}

async fn read_live<C: Manage>(engine: &mut C) -> Result<(Limits, BTreeMap<String, u64>), CliError> {
	let values = engine.get_settings().await?;
	let usage = engine.get_limit_usage().await?;
	Ok((Limits::from_map(&values), usage))
}
