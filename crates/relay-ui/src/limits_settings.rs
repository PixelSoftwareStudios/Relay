//! Storage and limits settings: the `limits.*` preferences the panel edits, their local
//! checks, the per-key outcome texts and the panel's UI-free state. Values are read with
//! `GetSettings` and written with `SetSettings`; the engine decides what is applied, saved or
//! rejected and the panel shows its answer. The ranges and defaults here mirror
//! `relay_config::prefs::limits::SPECS` (a test keeps them equal); the engine stays the judge.

use std::collections::BTreeMap;

use crate::controller::{ControlError, Controller};
use crate::engine::{SettingOutcome, SettingResult};
use crate::tr;

/// What a limit counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
	Mib,
	Entries,
	Days,
	Transfers,
	Calls,
}

impl Unit {
	/// The unit as shown after a number.
	pub fn text(self) -> &'static str {
		match self {
			Self::Mib => tr!("limits-unit-mib"),
			Self::Entries => tr!("limits-unit-entries"),
			Self::Days => tr!("limits-unit-days"),
			Self::Transfers => tr!("limits-unit-transfers"),
			Self::Calls => tr!("limits-unit-calls"),
		}
	}
}

/// One limit the panel edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Def {
	pub key: &'static str,
	pub unit: Unit,
	pub min: u32,
	pub max: u32,
	pub default: u32,
}

/// Number of limits.
pub const COUNT: usize = 7;

/// Every limit, in display order.
pub const DEFS: [Def; COUNT] = [
	Def {
		key: "limits.avatar_cache_mib",
		unit: Unit::Mib,
		min: 16,
		max: 2048,
		default: 128,
	},
	Def {
		key: "limits.messages_session_mib",
		unit: Unit::Mib,
		min: 8,
		max: 1024,
		default: 64,
	},
	Def {
		key: "limits.messages_retained_mib",
		unit: Unit::Mib,
		min: 8,
		max: 1024,
		default: 256,
	},
	Def {
		key: "limits.history_max_entries",
		unit: Unit::Entries,
		min: 0,
		max: 5000,
		default: 200,
	},
	Def {
		key: "limits.history_max_age_days",
		unit: Unit::Days,
		min: 1,
		max: 3650,
		default: 90,
	},
	Def {
		key: "limits.transfer_concurrency",
		unit: Unit::Transfers,
		min: 1,
		max: 8,
		default: 2,
	},
	Def {
		key: "limits.messages_retained_calls",
		unit: Unit::Calls,
		min: 0,
		max: 20,
		default: 3,
	},
];

impl Def {
	/// Plain-words name.
	pub fn label(&self) -> &'static str {
		match self.key {
			"limits.avatar_cache_mib" => tr!("limits-avatar-label"),
			"limits.messages_session_mib" => tr!("limits-session-label"),
			"limits.messages_retained_mib" => tr!("limits-retained-label"),
			"limits.history_max_entries" => tr!("limits-entries-label"),
			"limits.history_max_age_days" => tr!("limits-age-label"),
			"limits.messages_retained_calls" => tr!("limits-calls-label"),
			_ => tr!("limits-transfers-label"),
		}
	}

	/// What the limit does and what happens when it is reached.
	pub fn help(&self) -> &'static str {
		match self.key {
			"limits.avatar_cache_mib" => tr!("limits-avatar-help"),
			"limits.messages_session_mib" => tr!("limits-session-help"),
			"limits.messages_retained_mib" => tr!("limits-retained-help"),
			"limits.history_max_entries" => tr!("limits-entries-help"),
			"limits.history_max_age_days" => tr!("limits-age-help"),
			"limits.messages_retained_calls" => tr!("limits-calls-help"),
			_ => tr!("limits-transfers-help"),
		}
	}

	/// The line under the control for what is in use, when the engine reported it.
	pub fn usage_text(&self, bytes: u64) -> Option<String> {
		let used = format!("{:.1}", bytes as f64 / (1024.0 * 1024.0));
		match self.key {
			"limits.avatar_cache_mib" => Some(tr!("limits-avatar-used", used = used)),
			"limits.messages_session_mib" => Some(tr!("limits-session-used", used = used)),
			"limits.messages_retained_mib" => Some(tr!("limits-retained-used", used = used)),
			_ => None,
		}
	}
}

/// The values of all limits, in [`DEFS`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitValues(pub [u32; COUNT]);

impl Default for LimitValues {
	fn default() -> Self {
		Self(DEFS.map(|d| d.default))
	}
}

impl LimitValues {
	/// Read the keys out of a `GetSettings` dump; a missing or malformed key is an error
	/// rather than a silent default.
	pub fn from_dump(dump: &BTreeMap<String, String>) -> Result<Self, String> {
		let mut values = [0; COUNT];
		for (slot, def) in values.iter_mut().zip(&DEFS) {
			let text = dump
				.get(def.key)
				.ok_or_else(|| tr!("setting-unsupported").to_owned())?;
			*slot = text
				.parse()
				.map_err(|_| tr!("hotkey-bad-value", key = def.key, value = text.as_str()))?;
		}
		Ok(Self(values))
	}

	/// Local pre-check, so an obviously bad value never leaves the window.
	pub fn check(&self) -> Result<(), String> {
		for (def, value) in DEFS.iter().zip(self.0) {
			if !(def.min..=def.max).contains(&value) {
				return Err(tr!(
					"limits-check-range",
					label = def.label(),
					min = def.min,
					max = def.max,
					unit = def.unit.text()
				));
			}
		}
		if self.0[2] < self.0[1] {
			return Err(tr!("limits-check-pair").to_owned());
		}
		Ok(())
	}

	/// The keys that differ from `stored`, as `SetSettings` pairs.
	pub fn changes_from(&self, stored: &Self) -> Vec<(String, String)> {
		DEFS.iter()
			.zip(self.0.iter().zip(stored.0))
			.filter(|(_, (new, old))| **new != *old)
			.map(|(def, (new, _))| (def.key.to_owned(), new.to_string()))
			.collect()
	}
}

/// Plain-words name of a limit key, for the outcome list.
pub fn key_label(key: &str) -> &str {
	match DEFS.iter().find(|d| d.key == key) {
		Some(def) => def.label(),
		None => key,
	}
}

/// One line per key: what happened to it.
pub fn outcome_text(result: &SettingResult) -> String {
	let label = key_label(&result.key);
	let reason = crate::setting_support::reason(&result.reason);
	match (result.outcome, reason.is_empty()) {
		(SettingOutcome::AppliedLive, _) => tr!("limits-applied", label = label),
		(SettingOutcome::SavedNeedsRestart, true) => tr!("limits-saved", label = label),
		(SettingOutcome::SavedNeedsRestart, false) => {
			tr!("limits-saved-reason", label = label, reason = reason)
		}
		(SettingOutcome::Rejected, true) => tr!("limits-rejected", label = label),
		(SettingOutcome::Rejected, false) => {
			tr!("limits-rejected-reason", label = label, reason = reason)
		}
	}
}

/// What the daemon holds: the values and, where it knows them, what is in use (bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
	pub values: LimitValues,
	pub usage: BTreeMap<String, u64>,
}

/// UI-owned state of the panel.
#[derive(Debug, Clone, Default)]
pub struct LimitsPanel {
	/// A load has been started for this opening of the panel.
	pub requested: bool,
	/// What the daemon last reported.
	pub stored: Option<Result<Loaded, String>>,
	/// What the user is editing; seeded from the first successful load.
	pub draft: LimitValues,
	pub seeded: bool,
	/// A save is in flight.
	pub saving: bool,
	/// The daemon's answer to the last save, or why it failed.
	pub report: Option<Result<Vec<SettingResult>, String>>,
}

impl LimitsPanel {
	/// A load finished.
	pub fn loaded(&mut self, result: Result<Loaded, String>) {
		if let (Ok(loaded), false) = (&result, self.seeded) {
			self.draft = loaded.values;
			self.seeded = true;
		}
		self.stored = Some(result);
	}

	/// A save finished.
	pub fn saved(&mut self, result: Result<Vec<SettingResult>, String>) {
		self.saving = false;
		self.report = Some(result.map_err(crate::setting_support::friendly));
	}

	/// What the daemon holds, when it was read.
	pub fn current(&self) -> Option<LimitValues> {
		match &self.stored {
			Some(Ok(loaded)) => Some(loaded.values),
			_ => None,
		}
	}

	/// The draft differs from what the daemon holds.
	pub fn dirty(&self) -> bool {
		self.current().is_some_and(|c| c != self.draft)
	}

	/// The draft is not the defaults (so "Reset to defaults" has something to do).
	pub fn resettable(&self) -> bool {
		self.draft != LimitValues::default()
	}

	/// Drop the draft changes and the last report.
	pub fn revert(&mut self) {
		if let Some(current) = self.current() {
			self.draft = current;
		}
		self.report = None;
	}

	/// Put the defaults into the draft; nothing is sent until Save.
	pub fn reset_to_defaults(&mut self) {
		self.draft = LimitValues::default();
		self.report = None;
	}
}

impl Controller {
	/// Read the limits (and the usage the daemon reports) from the daemon.
	pub async fn limits_settings(&self) -> Result<Loaded, ControlError> {
		let session = self.slot.get().ok_or(ControlError::NotConnected)?;
		let dump = session.ops.get_settings().await?;
		let values = LimitValues::from_dump(&dump).map_err(ControlError::Engine)?;
		// Usage is a courtesy: without it the panel simply shows no usage.
		let usage = session.ops.limit_usage().await.unwrap_or_default();
		Ok(Loaded { values, usage })
	}

	/// Send the keys of `wanted` that differ from `stored`. Nothing differing sends nothing.
	/// The per-key answers are returned as the engine gave them.
	pub async fn save_limits(
		&self,
		stored: &LimitValues,
		wanted: &LimitValues,
	) -> Result<Vec<SettingResult>, ControlError> {
		wanted.check().map_err(ControlError::Engine)?;
		let changes = wanted.changes_from(stored);
		if changes.is_empty() {
			return Ok(Vec::new());
		}
		let session = self.slot.get().ok_or(ControlError::NotConnected)?;
		Ok(session.ops.set_settings(changes).await?)
	}
}
