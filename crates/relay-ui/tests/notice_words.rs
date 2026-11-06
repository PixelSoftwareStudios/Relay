//! The plain-language wording of the engine's warnings.

use relay_ui::i18n::{self, Language};
use relay_ui::model::Problem;
use relay_ui::notice_words::{Severity, clean, words};

fn problem(code: &str, message: &str, recovery: &str) -> Problem {
	Problem {
		code: code.into(),
		message: message.into(),
		retryable: false,
		recovery_action: recovery.into(),
	}
}

const KEY_CHANGED: &str = "`Dantes-Laptop.local` presented key 59F1-AAAA instead of the pinned 1A10-BBBB; the link was refused";

#[test]
fn the_key_mismatch_names_the_device_and_hides_the_digests() {
	i18n::set_active(Language::En);
	let w = words(&problem(
		"PEER_KEY_CHANGED",
		KEY_CHANGED,
		"verify_out_of_band",
	));
	assert_eq!(w.severity, Severity::Warning);
	assert_eq!(w.title, "Dantes-Laptop.local's security key changed");
	let step = w.next_step.expect("next step");
	assert!(step.starts_with("This usually happens after reinstalling or resetting Relay"));
	assert!(!w.title.contains("59F1") && !step.contains("59F1"));
	assert!(w.details.expect("details").contains("59F1-AAAA"));
}

#[test]
fn the_known_codes_have_words_in_english_german_and_russian() {
	let cases = [
		("PEER_KEY_CHANGED", KEY_CHANGED, ""),
		(
			"MEDIA_WORKER_FAILED",
			"The daemon's audio playback stopped responding. It was reset.",
			"",
		),
		(
			"MEDIA_WORKER_FAILED",
			"The daemon's audio playback failed.",
			"restart_daemon",
		),
		("MEDIA_WORKER_FAILED", "x", "restarted_render"),
		("MEDIA_WORKER_FAILED", "x", "restarted_capture"),
		("OUTPUT_DEVICE_LOST", "The playback device went away.", ""),
		(
			"MICROPHONE_UNAVAILABLE",
			"The microphone could not be opened (x).",
			"",
		),
		("AVATAR_REJECTED", "avatar from AB12 refused: nope", ""),
		("PREFERENCES_RECOVERED", "damaged", "review"),
		("PREFERENCES_RESET", "unreadable", "review"),
		("TRUST_RECOVERED", "damaged", "review"),
		("TRUST_LOST", "lost", "review"),
	];
	let mut seen = Vec::new();
	for language in [Language::En, Language::De, Language::Ru] {
		i18n::set_active(language);
		let mut titles = Vec::new();
		for (code, message, recovery) in cases {
			let w = words(&problem(code, message, recovery));
			assert!(!w.title.is_empty() && !w.title.contains("daemon"), "{code}");
			assert!(
				w.next_step.as_deref().is_some_and(|s| !s.is_empty()),
				"{code}"
			);
			assert!(
				!w.title.contains('`') && !w.title.contains("{ $"),
				"{code}: {}",
				w.title
			);
			titles.push(w.title);
		}
		seen.push(titles);
	}
	// Every language words every code, and the languages differ from English.
	for other in &seen[1..] {
		for (a, b) in seen[0].iter().zip(other) {
			assert_ne!(a, b);
		}
	}
	i18n::set_active(Language::En);
}

#[test]
fn audio_stalls_and_broken_audio_have_their_own_wording_and_severity() {
	i18n::set_active(Language::En);
	let stall = words(&problem(
		"MEDIA_WORKER_FAILED",
		"The daemon's audio playback stopped responding. It was reset.",
		"",
	));
	assert_eq!(stall.title, "Audio stopped for a moment and was restarted.");
	assert_eq!(stall.severity, Severity::Warning);
	let dead = words(&problem("MEDIA_WORKER_FAILED", "x", "restart_daemon"));
	assert_eq!(
		dead.title,
		"Audio stopped working and could not be restarted."
	);
	assert_eq!(dead.next_step.as_deref(), Some("Restart Relay."));
	assert_eq!(dead.severity, Severity::Warning, "never alarming");
	let render = words(&problem("MEDIA_WORKER_FAILED", "x", "restarted_render"));
	assert_eq!(render.title, "Audio from the other side stopped.");
	assert_eq!(render.next_step.as_deref(), Some("Relay restarted it."));
	let mic = words(&problem("MEDIA_WORKER_FAILED", "x", "restarted_capture"));
	assert_eq!(mic.title, "Your microphone stopped.");
	let info = words(&problem("AVATAR_REJECTED", "x", ""));
	assert_eq!(
		info.severity,
		Severity::Info,
		"informational, never warning-styled"
	);
}

#[test]
fn unknown_codes_show_a_cleaned_engine_message() {
	i18n::set_active(Language::En);
	let w = words(&problem(
		"NEW_THING",
		"invalid: the `daemon` could not reach Daemon x",
		"",
	));
	assert_eq!(
		w.title,
		"The background service could not reach Background service x"
	);
	assert_eq!(w.severity, Severity::Warning);
	assert_eq!(w.next_step, None);
	assert_eq!(clean("the daemon's disk"), "The background service's disk");
	assert_eq!(clean("daemonize it"), "Daemonize it", "whole words only");
}

#[test]
fn a_key_change_without_a_name_still_reads_well() {
	i18n::set_active(Language::En);
	let w = words(&problem("PEER_KEY_CHANGED", "key changed", ""));
	assert_eq!(w.title, "A device's security key changed");
}
