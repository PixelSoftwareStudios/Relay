//! The CLI against a real gRPC server over a Unix socket.

#![cfg(unix)]

mod common;

use common::engine::EngineHarness;
use common::fixtures::{ALICE, CALL};
use relay_api::pb::Problem;
use relay_cli::cli::exit;

#[tokio::test]
async fn status_and_peers_round_trip_json() {
	let h = EngineHarness::start().await;
	let out = h.run(&["--json", "status"]).await.unwrap();
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(v["engine_version"], "test-engine");
	assert_eq!(v["calls"][0]["state"], "connected");

	let out = h.run(&["--json", "peers"]).await.unwrap();
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(v.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn call_sends_the_peer_id_and_never_a_microphone_grant() {
	let h = EngineHarness::start().await;
	let out = h.run(&["call", "alice"]).await.unwrap();
	assert!(out.contains("applied"));
	assert_eq!(h.log(), [format!("start_call peer={}", hex::encode(ALICE))]);
}

#[tokio::test]
async fn call_mutations_acquire_a_lease_and_use_its_generation() {
	let h = EngineHarness::start().await;
	h.run(&["accept"]).await.unwrap();
	h.run(&["--takeover", "end", &hex::encode(&CALL[..4])])
		.await
		.unwrap();
	let log = h.log();
	assert_eq!(log[0], "acquire takeover=false");
	assert_eq!(log[1], "accept gen=5");
	assert_eq!(log[2], "acquire takeover=true");
	assert_eq!(log[3], format!("end call={}", hex::encode(CALL)));
}

#[tokio::test]
async fn talk_sends_the_mode_over_the_wire() {
	let h = EngineHarness::start().await;
	h.run(&["talk", "unmuted"]).await.unwrap();
	h.run(&["talk", "muted"]).await.unwrap();
	let log = h.log();
	assert!(log.contains(&"route mode=3".to_owned()), "{log:?}");
	assert!(log.contains(&"route mode=1".to_owned()), "{log:?}");
}

#[tokio::test]
async fn engine_rejection_maps_to_exit_code_four() {
	let h = EngineHarness::start().await;
	*h.engine.fail_with.lock().unwrap() = Some(Problem {
		code: "NO_SUCH_STATE".into(),
		message: "cannot do that now".into(),
		..Problem::default()
	});
	let err = h.run(&["end"]).await.unwrap_err();
	assert_eq!(err.exit_code(), exit::REJECTED);
	assert!(err.to_string().contains("cannot do that now"));
}

#[tokio::test]
async fn mic_test_reads_telemetry_and_stops() {
	let h = EngineHarness::start().await;
	let out = h
		.run(&["--json", "mic-test", "--seconds", "1"])
		.await
		.unwrap();
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(v["samples"], 3);
	assert_eq!(v["peak_dbfs"], -12.5);
	let log = h.log();
	assert_eq!(log.first().map(String::as_str), Some("mic_test_start"));
	assert_eq!(log.last().map(String::as_str), Some("mic_test_stop"));
}

#[tokio::test]
async fn missing_engine_is_unreachable_with_exit_code_three() {
	let dir = tempfile::tempdir().unwrap();
	let socket = dir.path().join("nothing.sock");
	use clap::Parser;
	let cli = relay_cli::cli::Cli::try_parse_from([
		"relay",
		"--socket",
		socket.to_str().unwrap(),
		"status",
	])
	.unwrap();
	let err = relay_cli::commands::run(cli, &mut Vec::new())
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::UNREACHABLE);
	assert_eq!(err.code(), "engine_unreachable");
}

#[tokio::test]
async fn doctor_writes_a_private_redacted_bundle_even_without_an_engine() {
	use std::os::unix::fs::PermissionsExt;
	let dir = tempfile::tempdir().unwrap();
	let socket = dir.path().join("nothing.sock");
	let bundle = dir.path().join("bundle.txt");
	use clap::Parser;
	let cli = relay_cli::cli::Cli::try_parse_from([
		"relay",
		"--json",
		"--socket",
		socket.to_str().unwrap(),
		"doctor",
		"--bundle",
		bundle.to_str().unwrap(),
	])
	.unwrap();
	let mut out = Vec::new();
	let err = relay_cli::commands::run(cli, &mut out).await.unwrap_err();
	assert_eq!(err.exit_code(), exit::FAILURE, "failed checks exit 1");
	let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
	assert_eq!(report["checks"][1]["name"], "engine");
	assert_eq!(report["checks"][1]["ok"], false);
	let text = std::fs::read_to_string(&bundle).unwrap();
	assert!(text.starts_with("# Relay support bundle"));
	assert_eq!(
		std::fs::metadata(&bundle).unwrap().permissions().mode() & 0o777,
		0o600
	);
}
