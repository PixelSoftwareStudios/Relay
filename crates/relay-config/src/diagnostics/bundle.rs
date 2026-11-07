//! Bounded, redacted support bundle.

use std::collections::BTreeMap;
use std::fmt::Write;

use super::redact::{fingerprint, redact_line};
use crate::notice::Notice;
use crate::prefs::Preferences;

/// Hard cap on bundle size.
pub const BUNDLE_MAX_BYTES: usize = 256 * 1024;
const MAX_LOG_LINES: usize = 500;
const MAX_LINE_CHARS: usize = 512;
const MAX_COUNTERS: usize = 256;

/// What the caller supplies. There is deliberately no field for keys, tokens,
/// trust entries, avatars or audio: they cannot end up in a bundle.
#[derive(Debug, Clone)]
pub struct BundleInput<'a> {
	pub app_version: &'a str,
	pub os: &'a str,
	pub preferences: &'a Preferences,
	pub notices: &'a [Notice],
	/// Numeric counters, e.g. packet loss and underrun totals.
	pub counters: &'a BTreeMap<String, u64>,
	/// Recent log lines, newest last.
	pub log_lines: &'a [String],
}

fn push_capped(out: &mut String, line: &str) -> bool {
	if out.len() + line.len() + 1 > BUNDLE_MAX_BYTES {
		return false;
	}
	out.push_str(line);
	out.push('\n');
	true
}

fn notice_label(n: &Notice) -> &'static str {
	match n {
		Notice::PreferencesRecoveredFromBackup => "preferences_recovered_from_backup",
		Notice::PreferencesReset { .. } => "preferences_reset",
		Notice::TrustRecoveredFromBackup => "trust_recovered_from_backup",
		Notice::TrustStoreLost { .. } => "trust_store_lost",
	}
}

/// Builds the bundle text. Display name and paths are omitted, device ids are
/// fingerprinted, and every log line is redacted.
pub fn build_bundle(input: &BundleInput<'_>) -> String {
	let p = input.preferences;
	let mut out = String::new();
	let _ = push_capped(&mut out, "# Relay support bundle (redacted)");
	let _ = push_capped(
		&mut out,
		&format!("app_version = {}", redact_line(input.app_version)),
	);
	let _ = push_capped(&mut out, &format!("os = {}", redact_line(input.os)));
	let _ = push_capped(&mut out, "\n[preferences]");
	let mut lines = String::new();
	let _ = writeln!(lines, "config_version = {}", p.config_version);
	let _ = writeln!(lines, "input_selection = {:?}", p.audio.input_selection);
	let _ = writeln!(
		lines,
		"input_device = {}",
		fingerprint(&p.audio.preferred_input_id)
	);
	let _ = writeln!(lines, "output_selection = {:?}", p.audio.output_selection);
	let _ = writeln!(
		lines,
		"output_device = {}",
		fingerprint(&p.audio.preferred_output_id)
	);
	let _ = writeln!(lines, "noise_suppression = {}", p.audio.noise_suppression);
	let _ = writeln!(
		lines,
		"noise_suppression_level = {:?}",
		p.audio.noise_suppression_level
	);
	let _ = writeln!(lines, "auto_gain = {}", p.audio.auto_gain);
	let _ = writeln!(lines, "microphone_gain = {}", p.audio.microphone_gain);
	let _ = writeln!(lines, "playback_gain = {}", p.audio.playback_gain);
	let _ = writeln!(
		lines,
		"default_transmit_mode = {:?}",
		p.calls.default_transmit_mode
	);
	let _ = writeln!(lines, "allow_wan = {}", p.network.allow_wan);
	let _ = writeln!(lines, "key_storage = {:?}", p.identity.key_storage);
	let _ = writeln!(
		lines,
		"raw_audio_capture_opt_in = {}",
		p.diagnostics.record_raw_audio
	);
	let _ = writeln!(lines, "raw_audio_included_in_bundle = false");
	for line in lines.lines() {
		let _ = push_capped(&mut out, line);
	}
	let _ = push_capped(&mut out, "\n[notices]");
	for n in input.notices {
		let _ = push_capped(&mut out, notice_label(n));
	}
	let _ = push_capped(&mut out, "\n[counters]");
	for (name, value) in input.counters.iter().take(MAX_COUNTERS) {
		let _ = push_capped(&mut out, &format!("{} = {value}", redact_line(name)));
	}
	let _ = push_capped(&mut out, "\n[log]");
	let skip = input.log_lines.len().saturating_sub(MAX_LOG_LINES);
	for raw in &input.log_lines[skip..] {
		let line: String = redact_line(raw).chars().take(MAX_LINE_CHARS).collect();
		if !push_capped(&mut out, &line) {
			let _ = push_capped(&mut out, "# truncated");
			break;
		}
	}
	out
}
