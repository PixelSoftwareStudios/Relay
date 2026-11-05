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
