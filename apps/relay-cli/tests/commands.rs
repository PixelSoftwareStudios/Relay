//! Command behavior against a scripted `Control`.

mod common;

use common::fixtures::{ALICE, CALL};
use common::{FakeControl, run_with};
use relay_api::pb::{OperationState, Problem};
use relay_cli::cli::exit;

fn call_hex() -> String {
	hex::encode(CALL)
}

#[tokio::test]
async fn peers_lists_names_trust_and_json() {
	let mut c = FakeControl::new();
	let text = run_with(&mut c, &["peers"], false).await.unwrap();
	assert!(text.contains("Alice") && text.contains("verified"));
	assert!(text.contains("Bob") && text.contains("unverified"));

	let json = run_with(&mut c, &["peers"], true).await.unwrap();
	let value: serde_json::Value = serde_json::from_str(&json).unwrap();
	assert_eq!(value[0]["name"], "Alice");
	assert_eq!(value[0]["id"], hex::encode(ALICE));
	assert_eq!(value[1]["reachable"], false);
}

#[tokio::test]
async fn status_reports_engine_calls_and_settings() {
	let mut c = FakeControl::new();
	let json = run_with(&mut c, &["status"], true).await.unwrap();
	let v: serde_json::Value = serde_json::from_str(&json).unwrap();
	assert_eq!(v["engine_version"], "9.9.9");
	assert_eq!(v["peers_online"], 1);
	assert_eq!(v["input_device"], "mic1");
	assert_eq!(v["calls"][0]["peer_name"], "Alice");
	assert_eq!(v["calls"][0]["mode"], "muted");
	assert_eq!(v["calls"][0]["transmitting"], false);
	let text = run_with(&mut c, &["status"], false).await.unwrap();
	assert!(text.contains("Alice") && text.contains("muted"));
}

#[tokio::test]
async fn call_resolves_name_prefix_and_manual_address() {
	let mut c = FakeControl::new();
	run_with(&mut c, &["call", "ali"], false).await.unwrap();
	run_with(&mut c, &["call", "10.0.0.5:7000"], false)
		.await
		.unwrap();
	assert!(c.log[0].contains("Peer"), "{:?}", c.log);
	assert!(c.log[1].contains("Address(\"10.0.0.5:7000\")"));
	let err = run_with(&mut c, &["call", "zed"], false).await.unwrap_err();
	assert_eq!(err.exit_code(), exit::TARGET);
}

#[tokio::test]
async fn lifecycle_uses_the_only_live_call() {
	let mut c = FakeControl::new();
	run_with(&mut c, &["end"], false).await.unwrap();
	run_with(&mut c, &["accept", "alice"], false).await.unwrap();
	assert_eq!(c.log[0], format!("End {}", call_hex()));
	assert_eq!(c.log[1], format!("Accept {}", call_hex()));

	c.snapshot = common::fixtures::no_calls();
	let err = run_with(&mut c, &["decline"], false).await.unwrap_err();
	assert_eq!(err.exit_code(), exit::TARGET);
}

#[tokio::test]
async fn talk_sets_the_mode_which_is_the_consent_in_one_request() {
	let mut c = FakeControl::new();
	run_with(&mut c, &["talk", "ptt"], false).await.unwrap();
	run_with(&mut c, &["talk", "unmuted"], false).await.unwrap();
	run_with(&mut c, &["talk", "muted"], false).await.unwrap();
	assert_eq!(c.log.len(), 3, "one request per choice: {:?}", c.log);
	assert!(c.log[0].contains("PushToTalk"));
	assert!(c.log[1].contains("Unmuted"));
	assert!(c.log[2].contains("Muted"));
	use clap::Parser;
	assert!(
		relay_cli::cli::Cli::try_parse_from(["relay", "talk", "ptt", "--consent"]).is_err(),
		"the consent flag is gone"
	);
}

#[tokio::test]
async fn mute_deafen_and_gain_target_global_or_call() {
	let mut c = FakeControl::new();
	run_with(&mut c, &["mute"], false).await.unwrap();
	run_with(&mut c, &["deafen", "off"], false).await.unwrap();
	run_with(&mut c, &["mute", "on", "--call", "alice"], false)
		.await
		.unwrap();
	run_with(&mut c, &["gain", "mic", "1.5"], false)
		.await
		.unwrap();
	run_with(&mut c, &["gain", "receive", "0.5"], false)
		.await
		.unwrap();
	assert!(c.log[0].contains("muted: Some(true)"));
	assert!(c.log[1].contains("deafened: Some(false)"));
	assert!(c.log[2].starts_with("call_controls"));
	assert!(c.log[3].contains("microphone_gain: Some(1.5)"));
	assert!(c.log[4].contains("receive_gain: Some(0.5)"));

	let err = run_with(&mut c, &["gain", "mic", "NaN"], false)
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
}

#[tokio::test]
async fn devices_select_resolves_names_and_keeps_the_other_side() {
	let mut c = FakeControl::new();
	let list = run_with(&mut c, &["devices", "list"], false).await.unwrap();
	let usb = list.lines().find(|l| l.contains("USB Mic")).unwrap();
	assert!(usb.split_whitespace().any(|w| w == "input"), "{list}");
	run_with(&mut c, &["devices", "select", "--input", "usb"], false)
		.await
		.unwrap();
	assert_eq!(c.log[0], "select_devices mic2 spk1");
	let err = run_with(&mut c, &["devices", "select"], false)
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
	let err = run_with(&mut c, &["devices", "select", "--output", "usb"], false)
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::TARGET);
}

#[tokio::test]
async fn ns_and_profile_pass_through() {
	let mut c = FakeControl::new();
	run_with(&mut c, &["ns", "set", "sonora/default"], false)
		.await
		.unwrap();
	run_with(&mut c, &["profile", "set", "--name", "Desk"], false)
		.await
		.unwrap();
	assert_eq!(c.log, ["ns sonora/default", "profile Desk"]);
	let err = run_with(&mut c, &["profile", "set", "--name", "  "], false)
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
}

#[tokio::test]
async fn trust_verify_needs_a_fingerprint_and_forget_clears() {
	let mut c = FakeControl::new();
	let bad = run_with(
		&mut c,
		&["trust", "verify", "bob", "--fingerprint", "abcd"],
		false,
	)
	.await;
	assert_eq!(bad.unwrap_err().exit_code(), exit::USAGE);
	let fp = "ab".repeat(32);
	run_with(
		&mut c,
		&["trust", "verify", "bob", "--fingerprint", &fp],
		false,
	)
	.await
	.unwrap();
	run_with(&mut c, &["trust", "forget", "alice"], false)
		.await
		.unwrap();
	assert!(
		c.log[0].contains("Verified") && c.log[0].contains("oob=true") && c.log[0].contains(&fp)
	);
	assert_eq!(c.log[1], "forget  alice");
	let list = run_with(&mut c, &["trust", "list"], false).await.unwrap();
	assert!(list.contains("Alice"));
}

#[tokio::test]
async fn trust_list_shows_the_short_fingerprint_and_forget_sends_the_query() {
	let mut c = FakeControl::new();
	c.snapshot.peers[0].fingerprint_short = "A1B2-C3D4-E5F6-0718-293A".into();
	let list = run_with(&mut c, &["trust", "list"], false).await.unwrap();
	assert!(list.contains("A1B2-C3D4-E5F6-0718-293A"), "{list}");
	let json = run_with(&mut c, &["trust", "list"], true).await.unwrap();
	assert!(json.contains("\"fingerprint_short\":\"A1B2-C3D4-E5F6-0718-293A\""));
	run_with(&mut c, &["trust", "forget", "a1b2-c3d4"], false)
		.await
		.unwrap();
	assert_eq!(c.log, ["forget  a1b2-c3d4"]);
	let blank = run_with(&mut c, &["trust", "forget", "  "], false).await;
	assert_eq!(blank.unwrap_err().exit_code(), exit::USAGE);
	assert_eq!(c.log.len(), 1);
}

#[tokio::test]
async fn failed_operation_becomes_a_rejection_with_the_engine_code() {
	let mut c = FakeControl::new();
	c.result.state = OperationState::Failed as i32;
	c.result.problem = Some(Problem {
		code: "CONTROLLER_HELD".into(),
		message: "another client controls this call".into(),
		retryable: false,
		recovery_action: String::new(),
	});
	let err = run_with(&mut c, &["end"], false).await.unwrap_err();
	assert_eq!(err.exit_code(), exit::REJECTED);
	assert_eq!(err.code(), "CONTROLLER_HELD");
}

#[tokio::test]
async fn mic_test_reports_peak_and_always_stops() {
	let mut c = FakeControl::new();
	c.telemetry = [-40.0, -9.0, -20.0]
		.into_iter()
		.map(|db| relay_api::pb::TelemetrySnapshot {
			microphone_dbfs: Some(db),
			..Default::default()
		})
		.collect();
	let out = run_with(&mut c, &["mic-test", "--seconds", "1"], true)
		.await
		.unwrap();
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(v["samples"], 3);
	assert_eq!(v["peak_dbfs"], -9.0);
	assert_eq!(
		c.log.first().map(String::as_str),
		Some("mic_test_start local=false confirmed=false")
	);
	assert_eq!(c.log.last().map(String::as_str), Some("mic_test_stop"));
}

#[tokio::test]
async fn connect_trust_sends_the_scanned_string_once() {
	let mut c = FakeControl::new();
	let text = format!("relay://192.168.1.20:47800#{}", "ab".repeat(32));
	run_with(&mut c, &["connect", "--trust", &text], false)
		.await
		.unwrap();
	assert_eq!(c.log, [format!("connect_and_trust {text}")]);
	use clap::Parser;
	assert!(
		relay_cli::cli::Cli::try_parse_from(["relay", "connect", &text]).is_err(),
		"the command exists to trust, so --trust is required"
	);
}

#[tokio::test]
async fn trust_replace_needs_a_changed_key_and_the_full_fingerprint() {
	let mut c = FakeControl::new();
	let fp = "59f1".repeat(16);
	// Alice has no key change: nothing is sent.
	let plain = run_with(
		&mut c,
		&["trust", "replace", "alice", "--fingerprint", &fp],
		false,
	)
	.await;
	assert_eq!(plain.unwrap_err().exit_code(), exit::USAGE);
	c.snapshot.peers[0].key_changed = true;
	c.snapshot.peers[0].previous_fingerprint_short = "1A10-1A10-1A10-1A10-1A10".into();
	let short = run_with(
		&mut c,
		&["trust", "replace", "alice", "--fingerprint", "59f1"],
		false,
	)
	.await;
	assert_eq!(short.unwrap_err().exit_code(), exit::USAGE);
	assert!(c.log.is_empty());
	let spaced = fp
		.as_bytes()
		.chunks(4)
		.map(|c| String::from_utf8_lossy(c).to_uppercase())
		.collect::<Vec<_>>()
		.join(" ");
	run_with(
		&mut c,
		&["trust", "replace", "alice", "--fingerprint", &spaced],
		false,
	)
	.await
	.unwrap();
	assert_eq!(c.log, [format!("replace {} {fp}", hex::encode(ALICE))]);
	let list = run_with(&mut c, &["peers"], false).await.unwrap();
	assert!(
		list.contains("KEY CHANGED") && list.contains("1A10-1A10"),
		"{list}"
	);
	let json = run_with(&mut c, &["peers"], true).await.unwrap();
	assert!(json.contains("\"key_changed\":true"), "{json}");
}
