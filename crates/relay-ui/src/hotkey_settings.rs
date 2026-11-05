//! Push-to-talk hotkey settings: the two preferences the panel edits, their
//! local checks, the per-key outcome texts and the panel's UI-free state. The
//! values are read with `GetSettings` and written with `SetSettings`; the
//! engine decides what is applied, saved or rejected and the panel only shows
//! its answer.

use std::collections::BTreeMap;

use crate::controller::{ControlError, Controller, RELEASE_DELAY_MAX_MS};
use crate::engine::{SettingOutcome, SettingResult};

/// Global push-to-talk accelerator (read when the hotkey backend starts).
pub const KEY_ACCELERATOR: &str = "hotkeys.push_to_talk";
/// How long the gate stays open after release.
pub const KEY_RELEASE_DELAY: &str = "ptt.release_delay_ms";
/// Longest accelerator text the daemon accepts.
pub const ACCELERATOR_MAX_CHARS: usize = 64;

/// What the panel edits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HotkeyValues {
	/// Empty disables the global hotkey.
	pub accelerator: String,
	pub release_delay_ms: u32,
}

impl HotkeyValues {
	/// Read the two keys out of a `GetSettings` dump; a missing or malformed
	/// key is an error rather than a silent default.
	pub fn from_dump(dump: &BTreeMap<String, String>) -> Result<Self, String> {
		let get = |key: &str| {
			dump.get(key)
				.ok_or_else(|| format!("the daemon did not report {key}"))
		};
		let delay = get(KEY_RELEASE_DELAY)?;
		Ok(Self {
			accelerator: get(KEY_ACCELERATOR)?.clone(),
			release_delay_ms: delay
				.parse()
				.map_err(|_| format!("the daemon reported a bad {KEY_RELEASE_DELAY}: {delay}"))?,
		})
	}

	/// Local pre-check, so an obviously bad value never leaves the window.
	pub fn check(&self) -> Result<(), &'static str> {
		let text = &self.accelerator;
		if text.chars().count() > ACCELERATOR_MAX_CHARS || text.chars().any(char::is_control) {
			return Err("the hotkey must be at most 64 printable characters");
		}
		if self.release_delay_ms > RELEASE_DELAY_MAX_MS {
			return Err("release delay must be between 0 and 2000 ms");
		}
		Ok(())
	}

	/// The keys that differ from `stored`, as `SetSettings` pairs.
	pub fn changes_from(&self, stored: &Self) -> Vec<(String, String)> {
		let mut out = Vec::new();
		if self.accelerator != stored.accelerator {
			out.push((KEY_ACCELERATOR.to_owned(), self.accelerator.clone()));
		}
		if self.release_delay_ms != stored.release_delay_ms {
			out.push((
				KEY_RELEASE_DELAY.to_owned(),
				self.release_delay_ms.to_string(),
			));
		}
		out
	}
}

/// Plain-words name of a key, for the outcome list.
pub fn key_label(key: &str) -> &str {
	match key {
		KEY_ACCELERATOR => "Push-to-talk hotkey",
		KEY_RELEASE_DELAY => "Release delay",
		other => other,
	}
}

/// One line per key: what happened to it.
pub fn outcome_text(result: &SettingResult) -> String {
	let label = key_label(&result.key);
	let reason = result.reason.trim();
	match (result.outcome, reason.is_empty()) {
		(SettingOutcome::AppliedLive, _) => format!("{label}: applied now"),
		(SettingOutcome::SavedNeedsRestart, true) => {
			format!("{label}: saved, takes effect after a restart")
		}
		(SettingOutcome::SavedNeedsRestart, false) => {
			format!("{label}: saved, takes effect after a restart ({reason})")
		}
		(SettingOutcome::Rejected, true) => format!("{label}: rejected"),
		(SettingOutcome::Rejected, false) => format!("{label}: rejected ({reason})"),
	}
}

/// UI-owned state of the panel (kept in `Local`).
#[derive(Debug, Default)]
pub struct HotkeyPanel {
	/// A load has been started for this opening of the panel.
	pub requested: bool,
	/// What the daemon last reported.
	pub stored: Option<Result<HotkeyValues, String>>,
	/// What the user is editing; seeded from the first successful load.
	pub draft: HotkeyValues,
	pub seeded: bool,
	/// A save is in flight.
	pub saving: bool,
	/// The daemon's answer to the last save, or why it failed.
	pub report: Option<Result<Vec<SettingResult>, String>>,
}

impl HotkeyPanel {
	/// A load finished.
	pub fn loaded(&mut self, result: Result<HotkeyValues, String>) {
		if let (Ok(values), false) = (&result, self.seeded) {
			self.draft = values.clone();
			self.seeded = true;
		}
		self.stored = Some(result);
	}

	/// A save finished.
	pub fn saved(&mut self, result: Result<Vec<SettingResult>, String>) {
		self.saving = false;
		self.report = Some(result);
	}

	/// The draft differs from what the daemon holds.
	pub fn dirty(&self) -> bool {
		matches!(&self.stored, Some(Ok(s)) if *s != self.draft)
	}

	/// Drop the draft and the last report.
	pub fn revert(&mut self) {
		if let Some(Ok(s)) = &self.stored {
			self.draft = s.clone();
		}
		self.report = None;
	}

	/// The panel closed: forget everything so a reopen asks the daemon again.
	pub fn reset(&mut self) {
		*self = Self::default();
	}
}

impl Controller {
	/// Read the hotkey preferences from the daemon.
	pub async fn hotkey_settings(&self) -> Result<HotkeyValues, ControlError> {
		let session = self.slot.get().ok_or(ControlError::NotConnected)?;
		let dump = session.ops.get_settings().await?;
		HotkeyValues::from_dump(&dump).map_err(ControlError::Engine)
	}

	/// Send the keys of `wanted` that differ from `stored`. Nothing differing
	/// sends nothing. The per-key answers are returned as the engine gave them.
	pub async fn save_hotkey_settings(
		&self,
		stored: &HotkeyValues,
		wanted: &HotkeyValues,
	) -> Result<Vec<SettingResult>, ControlError> {
		wanted.check().map_err(ControlError::Invalid)?;
		let changes = wanted.changes_from(stored);
		if changes.is_empty() {
			return Ok(Vec::new());
		}
		let session = self.slot.get().ok_or(ControlError::NotConnected)?;
		Ok(session.ops.set_settings(changes).await?)
	}
}
