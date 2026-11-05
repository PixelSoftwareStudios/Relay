//! Message log times: local clock in text, UTC milliseconds kept in `--json`.

mod common;

use common::chat::{BASE_MS, SESSION_A};
use common::{FakeControl, run_messages};

fn chat() -> FakeControl {
	let mut c = FakeControl::new();
	c.chat.incoming_text(&SESSION_A, "hello from Alice");
	c
}

#[tokio::test]
async fn text_log_shows_the_injected_zone() {
	let mut c = chat();
	let out = run_messages(&mut c, &["log"], false).await.ok();
	// The test zone is UTC+02:00; BASE_MS + 1 s is 22:13:21 UTC.
	assert!(out.contains("00:13:21"), "{out}");
	assert!(!out.contains("22:13:21"), "{out}");
	assert!(!out.contains("00:13:21Z"), "{out}");
}

#[tokio::test]
async fn json_keeps_the_utc_instant_and_adds_no_local_field() {
	let mut c = chat();
	let out = run_messages(&mut c, &["log"], true).await.ok();
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(v[0]["time_unix_ms"], BASE_MS + 1000);
	assert!(v[0].get("local_clock").is_none(), "{out}");
}
