//! `relay auto-answer [list|on|off]` against the scripted control, and `relay status` showing
//! the background and call settings.

mod common;

use common::{FakeControl, run_with};
use relay_cli::cli::exit;

#[tokio::test]
async fn on_asks_the_engine_for_a_verified_contact_only() {
	let mut c = FakeControl::new();
	run_with(&mut c, &["auto-answer", "on", "alice"], false)
		.await
		.unwrap();
	assert_eq!(c.log, ["auto_answer aa true"]);
	assert!(c.snapshot.peers[0].auto_answer);
	// Bob is only trusted on first use: refused before anything is sent.
	let err = run_with(&mut c, &["auto-answer", "on", "bob"], false)
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
	assert!(err.to_string().contains("not verified"), "{err}");
	assert_eq!(c.log.len(), 1);
}

#[tokio::test]
async fn off_and_list_follow_the_flags() {
	let mut c = FakeControl::new();
	let empty = run_with(&mut c, &["auto-answer"], false).await.unwrap();
	assert!(empty.contains("relay auto-answer on"), "{empty}");
	run_with(&mut c, &["auto-answer", "on", "alice"], false)
		.await
		.unwrap();
	let json: serde_json::Value = serde_json::from_str(
		&run_with(&mut c, &["auto-answer", "list"], true)
			.await
			.unwrap(),
	)
	.unwrap();
	assert_eq!(json["enabled"], true);
	assert_eq!(json["contacts"][0]["name"], "Alice");
	run_with(&mut c, &["auto-answer", "off", "alice"], false)
		.await
		.unwrap();
	assert_eq!(c.log[1], "auto_answer aa false");
	let none = run_with(&mut c, &["auto-answer", "list"], true)
		.await
		.unwrap();
	assert!(
		none.contains("\"contacts\": []") || none.contains("\"contacts\":[]"),
		"{none}"
	);
	let unknown = run_with(&mut c, &["auto-answer", "on", "nobody"], false).await;
	assert_eq!(unknown.unwrap_err().exit_code(), exit::TARGET);
}

#[tokio::test]
async fn the_list_says_when_the_global_switch_is_off() {
	let mut c = FakeControl::new();
	c.snapshot.settings.as_mut().unwrap().auto_answer_enabled = false;
	let text = run_with(&mut c, &["auto-answer"], false).await.unwrap();
	assert!(text.contains("switched off"), "{text}");
}

#[tokio::test]
async fn status_shows_background_alert_and_auto_answer() {
	let mut c = FakeControl::new();
	let text = run_with(&mut c, &["status"], false).await.unwrap();
	assert!(text.contains("stays available"), "{text}");
	assert!(text.contains("alert"), "{text}");
	assert!(text.contains("auto-answer"), "{text}");
	let json: serde_json::Value =
		serde_json::from_str(&run_with(&mut c, &["status"], true).await.unwrap()).unwrap();
	assert_eq!(json["background_available"], true);
	assert_eq!(json["call_alert"], "system");
	assert_eq!(json["auto_answer_enabled"], true);
}

#[cfg(unix)]
#[tokio::test]
async fn the_new_keys_round_trip_through_a_real_engine() {
	let h = common::engine::EngineHarness::start().await;
	let listed = h.run(&["auto-answer"]).await.unwrap();
	assert!(listed.contains("relay auto-answer on"), "{listed}");
}
