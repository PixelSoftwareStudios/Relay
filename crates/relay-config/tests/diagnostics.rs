use std::collections::BTreeMap;

use relay_config::Notice;
use relay_config::diagnostics::{
	BUNDLE_MAX_BYTES, BundleInput, RawCaptureConsent, build_bundle, redact_line,
};
use relay_config::prefs::Preferences;

fn bundle_for(prefs: &Preferences, logs: &[String]) -> String {
	let counters = BTreeMap::from([("underruns".to_owned(), 3u64)]);
	build_bundle(&BundleInput {
		app_version: "0.1.0",
		os: "test-os",
		preferences: prefs,
		notices: &[Notice::PreferencesRecoveredFromBackup],
		counters: &counters,
		log_lines: logs,
	})
}

#[test]
fn bundle_omits_identity_paths_and_names() {
	let mut prefs = Preferences::default();
	prefs.profile.display_name = "Secret Person".into();
	prefs.audio.preferred_input_id = "Alice's USB Mic".into();
	let out = bundle_for(&prefs, &[]);
	assert!(!out.contains("Secret Person"));
	assert!(!out.contains("Alice"));
	assert!(out.contains("input_device = id:"));
	assert!(out.contains("preferences_recovered_from_backup"));
	assert!(out.contains("underruns = 3"));
	assert!(out.contains("raw_audio_included_in_bundle = false"));
}

#[test]
fn log_lines_are_redacted() {
	let key = "ab".repeat(32);
	let logs = vec![
		format!("peer key {key} connected from 192.168.1.20:5000"),
		"loaded /Users/alice/Library/Relay/config.toml ok".to_owned(),
		"auth_token=hunter2 other=fine".to_owned(),
		"Authorization: Bearer abc.def.ghi".to_owned(),
		"C:\\Users\\bob\\data\\id.key".to_owned(),
	];
	let out = bundle_for(&Preferences::default(), &logs);
	for leaked in [
		key.as_str(),
		"192.168",
		"alice",
		"hunter2",
		"abc.def",
		"bob",
	] {
		assert!(!out.contains(leaked), "leaked {leaked}:\n{out}");
	}
	assert!(out.contains("other=fine"));
}

#[test]
fn redact_line_keeps_ordinary_text() {
	assert_eq!(
		redact_line("jitter buffer grew to 40 ms"),
		"jitter buffer grew to 40 ms"
	);
}

#[test]
fn bundle_is_bounded() {
	let line = "x".repeat(400);
	let logs: Vec<String> = (0..5000).map(|_| line.clone()).collect();
	let out = bundle_for(&Preferences::default(), &logs);
	assert!(out.len() <= BUNDLE_MAX_BYTES);
	assert!(out.lines().count() < 700);
}

#[test]
fn raw_capture_needs_explicit_opt_in() {
	let mut prefs = Preferences::default();
	assert!(RawCaptureConsent::from_preferences(&prefs).is_none());
	prefs.diagnostics.record_raw_audio = true;
	assert!(RawCaptureConsent::from_preferences(&prefs).is_some());
}

#[test]
fn every_ip_form_is_redacted() {
	for ip in [
		"10.0.0.1",
		"192.168.1.20:5000",
		"fe80::1",
		"fd00::1",
		"::1",
		"fd12::5",
		"2001:db8::8a2e:370:7334",
		"2001:db8:0:0:0:0:2:1",
		"::ffff:192.168.1.20",
		"fe80::1%en0",
		"fe80::1%2",
		"[fd00::1]",
		"[fd00::1]:7000",
		"[fe80::1%en0]:7000",
	] {
		let plain = redact_line(&format!("connected to {ip} now"));
		assert_eq!(plain, "connected to <redacted> now", "{ip}");
		let keyed = redact_line(&format!("addr={ip}"));
		assert_eq!(keyed, "addr=<redacted>", "{ip}");
		let labelled = redact_line(&format!("peer:{ip}"));
		assert_eq!(labelled, "peer:<redacted>", "{ip}");
		let quoted = redact_line(&format!("(\"{ip}\"),"));
		assert!(!quoted.contains(ip), "{ip}: {quoted}");
	}
}

#[test]
fn ordinary_text_is_not_over_redacted() {
	for line in [
		"jitter buffer grew to 40 ms",
		"started at 12:30",
		"ratio 3:1 reached",
		"version 1.2.3",
		"std::sync::Mutex poisoned",
		"separator :: here",
		"call [ok] done",
		"time 10:20:30",
		"list [1, 2, 3]",
		"path-less thing: fine",
	] {
		assert_eq!(redact_line(line), line);
	}
}

#[test]
fn short_fingerprints_after_identifying_keys_are_redacted() {
	for line in [
		"fingerprint=A1B2C3D4",
		"fingerprint=A1B2-C3D4-E5F6",
		"peer=1a2b3c4d5e",
		"call from peer:1a2b3c4d",
		"new device fingerprint A1B2-C3D4 added",
		"fp A1B2C3D4",
	] {
		let out = redact_line(line);
		assert!(out.contains("<redacted>"), "{line} -> {out}");
		assert!(
			!out.to_ascii_lowercase().contains("a1b2") && !out.contains("1a2b3c"),
			"{out}"
		);
	}
	// Ordinary words and unrelated hex stay.
	assert_eq!(redact_line("peer connected"), "peer connected");
	assert_eq!(redact_line("peer added"), "peer added");
	assert_eq!(redact_line("color=DEADBEEF"), "color=DEADBEEF");
	assert_eq!(redact_line("fingerprint=abc"), "fingerprint=abc");
}

#[test]
fn mac_addresses_and_local_host_names_are_redacted() {
	let out = redact_line("iface aa:bb:cc:dd:ee:ff up, also AA-BB-CC-DD-EE-FF");
	assert_eq!(out, "iface <redacted> up, also <redacted>");
	assert_eq!(redact_line("mac=aa:bb:cc:dd:ee:ff"), "mac=<redacted>");
	for line in [
		"resolved ann-laptop.local now",
		"host=Anns-MacBook.local.",
		"to ann-pc.local:47800 ok",
	] {
		let out = redact_line(line);
		assert!(
			out.contains("<redacted>") && !out.to_lowercase().contains("ann"),
			"{line} -> {out}"
		);
	}
	// Not a host name or a MAC.
	assert_eq!(redact_line("just .local"), "just .local");
	assert_eq!(redact_line("1:2:3:4:5:6"), "1:2:3:4:5:6");
	assert_eq!(redact_line("version 1.2.3"), "version 1.2.3");
}
