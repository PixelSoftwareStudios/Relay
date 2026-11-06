//! `relay stats` and `relay calls`: diagnostics and the connected time of live calls.

mod common;

use common::fixtures::CALL;
use common::{FakeControl, run_with};
use relay_api::pb::{CallStats, MediaPathState};

fn sample() -> CallStats {
	CallStats {
		call_id: CALL.to_vec(),
		path_state: MediaPathState::Connected as i32,
		sending: true,
		receiving: true,
		packets_sent: 120,
		packets_received: 118,
		rtt_ms: Some(41.5),
		jitter_ms: Some(3.2),
		loss_percent_5s: 1.5,
		mouth_to_ear_ms: Some(96.0),
		local_addr: "192.168.1.20:5000".into(),
		remote_addr: "192.168.1.21:5001".into(),
		transport: "lan".into(),
		..CallStats::default()
	}
}

#[tokio::test]
async fn stats_prints_the_first_sample_as_text_and_json() {
	let mut c = FakeControl::new();
	c.call_stats = vec![sample()];
	let text = run_with(&mut c, &["stats"], false).await.unwrap();
	assert!(
		text.contains("connected") && text.contains("41.5 ms"),
		"{text}"
	);
	assert!(text.contains("192.168.1.21:5001"));
	let json = run_with(&mut c, &["stats"], true).await.unwrap();
	let value: serde_json::Value = serde_json::from_str(&json).unwrap();
	assert_eq!(value["sending"], true);
	assert_eq!(value["packets_received"], 118);
	assert_eq!(value["path"], "connected");
}

#[tokio::test]
async fn stats_watch_prints_every_sample() {
	let mut c = FakeControl::new();
	c.call_stats = vec![sample(), sample(), sample()];
	let json = run_with(&mut c, &["stats", "--watch"], true).await.unwrap();
	assert_eq!(json.lines().count(), 3);
}

#[tokio::test]
async fn calls_json_has_connected_at_duration_and_diagnostics() {
	let mut c = FakeControl::new();
	c.call_stats = vec![sample()];
	let connected_at = relay_io::clock::unix_millis() - 65_000;
	c.snapshot.calls[0].connected_at_unix_ms = connected_at;
	let json = run_with(&mut c, &["calls"], true).await.unwrap();
	let value: serde_json::Value = serde_json::from_str(&json).unwrap();
	let call = &value["calls"][0];
	assert_eq!(call["connected_at_unix_ms"], connected_at);
	let duration = call["duration_ms"].as_u64().unwrap();
	assert!((65_000..70_000).contains(&duration), "{duration}");
	assert_eq!(call["diagnostics"]["path"], "connected");
	let text = run_with(&mut c, &["calls"], false).await.unwrap();
	assert!(text.contains("1:0"), "duration column: {text}");
}

#[tokio::test]
async fn a_call_that_is_not_connected_has_no_duration() {
	let mut c = FakeControl::new();
	c.snapshot.calls[0].state = relay_api::pb::CallState::OutgoingRinging as i32;
	let json = run_with(&mut c, &["calls"], true).await.unwrap();
	let value: serde_json::Value = serde_json::from_str(&json).unwrap();
	assert!(value["calls"][0]["duration_ms"].is_null());
	assert!(value["calls"][0].get("diagnostics").is_none());
}
