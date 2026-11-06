//! Only `relay daemon stop` asks for the admin capability.
#![cfg(unix)]

mod common;

use common::engine::EngineHarness;

#[tokio::test]
async fn admin_is_requested_for_stop_and_for_nothing_else() {
	let h = EngineHarness::start().await;
	h.run(&["status"]).await.expect("status");
	h.run(&["mute", "on"]).await.expect("mute");
	let asked = h.engine.registered.lock().unwrap().clone();
	assert!(!asked.is_empty());
	assert!(
		asked.iter().all(|caps| !caps.iter().any(|c| c == "admin")),
		"{asked:?}"
	);

	// The stand-in engine keeps serving after Shutdown, so the wait runs out.
	let result = h.run(&["daemon", "stop", "--wait", "1"]).await;
	assert!(result.is_err(), "the endpoint never went away");
	assert!(h.log().contains(&"shutdown".to_owned()), "{:?}", h.log());
	let asked = h.engine.registered.lock().unwrap().clone();
	assert_eq!(asked.last().expect("a registration"), &["admin".to_owned()]);
}

#[tokio::test]
async fn status_shows_the_daemon_build_and_flags_a_mismatch() {
	let h = EngineHarness::start().await;
	let text = h.run(&["daemon", "status"]).await.expect("status");
	let older = relay_api::API_REVISION - 1;
	assert!(text.contains(&format!("build test (api {older})")), "{text}");
	assert!(
		text.contains("out of date"),
		"the stand-in is another build: {text}"
	);
	let json = h.run(&["--json", "daemon", "status"]).await.expect("json");
	let value: serde_json::Value = serde_json::from_str(&json).unwrap();
	assert_eq!(value["build_revision"], "test");
	assert_eq!(value["api_revision"], older);
	assert!(value["out_of_date"].is_string(), "{value}");
}

#[tokio::test]
async fn restart_refuses_while_a_call_is_in_progress() {
	let h = EngineHarness::start().await;
	let refused = h.run(&["daemon", "restart"]).await;
	assert!(refused.is_err(), "the fixture has a connected call");
	assert!(
		!h.log().contains(&"shutdown".to_owned()),
		"nothing was stopped: {:?}",
		h.log()
	);
}
