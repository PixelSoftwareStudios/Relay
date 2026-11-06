//! `relay history [--limit N] [--json]` and `relay history clear` against a scripted engine.

mod common;

use clap::Parser;
use common::engine::EngineHarness;
use common::{FakeControl, run_with};
use relay_api::pb::{CallDirection, CallHistoryEntry, CallOutcome};
use relay_cli::cli::Cli;

fn entry(id: u64, outcome: CallOutcome, ms: u64) -> CallHistoryEntry {
	CallHistoryEntry {
		id,
		call_id: vec![id as u8; 16],
		peer_name: format!("Peer {id}"),
		peer_id: vec![0xab; 32],
		peer_fingerprint_short: "ABAB-ABAB-ABAB-ABAB-ABAB".into(),
		direction: CallDirection::Outgoing as i32,
		outcome: outcome as i32,
		started_at_unix_ms: 1_800_000_000_000,
		duration_ms: ms,
	}
}

fn scripted() -> FakeControl {
	let mut c = FakeControl::new();
	c.history = vec![
		entry(3, CallOutcome::Completed, 3_725_000),
		entry(2, CallOutcome::Missed, 0),
		entry(1, CallOutcome::Failed, 0),
	];
	c
}

#[tokio::test]
async fn json_lists_every_field_with_ids_as_hex() {
	let mut c = scripted();
	let out = run_with(&mut c, &["history"], true).await.unwrap();
	let rows: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(rows.as_array().unwrap().len(), 3);
	let first = &rows[0];
	assert_eq!(first["call_id"], "03".repeat(16));
	assert_eq!(first["peer_id"], "ab".repeat(32));
	assert_eq!(first["peer_name"], "Peer 3");
	assert_eq!(first["direction"], "outgoing");
	assert_eq!(first["outcome"], "completed");
	assert_eq!(first["duration_ms"], 3_725_000);
	assert_eq!(first["started_at_unix_ms"], 1_800_000_000_000u64);
	assert_eq!(rows[1]["outcome"], "missed");
	assert_eq!(rows[2]["outcome"], "failed");
	assert_eq!(c.log, ["call_history 20 None"]);
}

#[tokio::test]
async fn the_limit_is_passed_on_and_text_shows_a_table() {
	let mut c = scripted();
	let out = run_with(&mut c, &["history", "--limit", "2"], false)
		.await
		.unwrap();
	assert_eq!(c.log, ["call_history 2 None"]);
	assert!(out.contains("Peer 3") && out.contains("Peer 2"), "{out}");
	assert!(!out.contains("Peer 1"), "{out}");
	assert!(out.contains("1:02:05"), "{out}");
	assert!(out.contains("missed"), "{out}");
}

#[tokio::test]
async fn an_empty_history_says_so_and_clear_deletes() {
	let mut c = FakeControl::new();
	let out = run_with(&mut c, &["history"], false).await.unwrap();
	assert_eq!(out.trim(), "no call history");
	let mut c = scripted();
	run_with(&mut c, &["history", "clear"], false)
		.await
		.unwrap();
	assert_eq!(c.log, ["clear_call_history"]);
	assert!(c.history.is_empty());
}

#[test]
fn the_limit_must_be_within_range() {
	assert!(Cli::try_parse_from(["relay", "history", "--limit", "0"]).is_err());
	assert!(Cli::try_parse_from(["relay", "history", "--limit", "201"]).is_err());
	assert!(Cli::try_parse_from(["relay", "history", "--limit", "200", "--json"]).is_ok());
}

#[tokio::test]
async fn clear_reaches_a_socket_engine() {
	let harness = EngineHarness::start().await;
	let out = harness.run(&["history", "clear"]).await;
	assert!(out.is_ok(), "{out:?}");
	assert!(harness.log().iter().any(|l| l == "clear_call_history"));
}
