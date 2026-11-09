//! `relay channel nearby`: the listed channels other devices advertise, as a table and JSON.

mod common;

use common::{FakeControl, run_with};
use relay_api::pb::NearbyChannel;

fn nearby(name: &str, devices: u32, ambiguous: bool) -> NearbyChannel {
	NearbyChannel {
		name: name.into(),
		devices,
		ambiguous,
	}
}

#[tokio::test]
async fn nearby_lists_names_with_device_counts_and_flags_a_clash() {
	let mut c = FakeControl::new();
	c.snapshot.nearby_channels = vec![nearby("Garage", 2, false), nearby("Lobby", 1, true)];
	let text = run_with(&mut c, &["channel", "nearby"], false)
		.await
		.unwrap();
	assert!(
		text.contains("Garage") && text.contains("DEVICES"),
		"{text}"
	);
	assert!(
		text.contains("Lobby") && text.contains("ambiguous"),
		"{text}"
	);

	let json = run_with(&mut c, &["channel", "nearby"], true)
		.await
		.unwrap();
	let v: serde_json::Value = serde_json::from_str(&json).unwrap();
	assert_eq!(v[0]["name"], "Garage");
	assert_eq!(v[0]["devices"], 2);
	assert_eq!(v[0]["ambiguous"], false);
	assert_eq!(v[1]["ambiguous"], true);
	assert!(
		v[0].get("id").is_none() && v[0].get("channel_id").is_none(),
		"the id is not exposed"
	);
}

#[tokio::test]
async fn nothing_nearby_is_one_plain_line_and_an_empty_json_list() {
	let mut c = FakeControl::new();
	let text = run_with(&mut c, &["channel", "nearby"], false)
		.await
		.unwrap();
	assert_eq!(text.trim(), "no channels found on your network");
	let json = run_with(&mut c, &["channel", "nearby"], true)
		.await
		.unwrap();
	assert_eq!(json.trim(), "[]");
}
