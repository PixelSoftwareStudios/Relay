#![cfg(unix)]
//! The background and call-alert settings go live and show in the snapshot.

#[macro_use]
mod service_support;

use relay_engine::service::is_live;
use service_support::*;

#[test]
fn the_new_keys_are_live() {
	for key in [
		"background.available",
		"calls.alert",
		"calls.auto_answer_enabled",
	] {
		assert!(is_live(key), "{key}");
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_snapshot_carries_the_settings_and_changes_apply_live() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("ui").await;
	let before = cli.snapshot().await.unwrap().settings.unwrap();
	assert!(before.background_available);
	assert_eq!(before.call_alert, "system");
	assert!(before.auto_answer_enabled);
	let results = cli
		.set_settings([
			("background.available", "false"),
			("calls.alert", "ring_and_vibrate"),
			("calls.auto_answer_enabled", "false"),
		])
		.await
		.unwrap();
	assert_eq!(results.results.len(), 3);
	let after = eventually!(
		cli.snapshot()
			.await
			.unwrap()
			.settings
			.filter(|s| s.call_alert == "ring_and_vibrate")
	);
	assert!(!after.background_available);
	assert!(!after.auto_answer_enabled);
}
