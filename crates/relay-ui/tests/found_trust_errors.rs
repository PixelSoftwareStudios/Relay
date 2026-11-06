//! A trust decision the engine could not store shows a notice in plain words, and the
//! device stays under Nearby; a stored one moves the device into Contacts.

mod common;
mod phone;

use std::sync::Arc;

use common::FakeOps;
use phone::Phone;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::{Peer, Trust};

fn stranger() -> Peer {
	Peer {
		peer_id: "ab".repeat(32),
		display_name: "Dantes Laptop".into(),
		addresses: vec!["192.168.1.77:7000".into()],
		trust: Trust::Unknown,
		reachable: false,
		avatar_hash: String::new(),
		fingerprint_short: "ABAB-ABAB-ABAB-ABAB-ABAB".into(),
		shared_channels: Vec::new(),
		favorite: false,
		favorite_position: 0,
		key_changed: false,
		previous_peer_id: String::new(),
		previous_fingerprint_short: String::new(),
		port: 7000,
		last_seen_unix_ms: 0,
	}
}

fn rig(code: Option<&str>) -> (Phone, Arc<FakeOps>) {
	let mut phone = Phone::desktop(720.0, 1800.0, Language::En);
	let ops = Arc::new(FakeOps::default());
	if let Some(code) = code {
		*ops.fail_with.lock().unwrap() = Some("unavailable: no authenticated key".into());
		*ops.fail_code.lock().unwrap() = Some(code.into());
	}
	phone.slot.set(ops.clone());
	let mut state = phone::busy_state();
	state.peers.push(stranger());
	phone.show_state(&state);
	phone.actions.hub.update(|s| s.peers.push(stranger()));
	phone.page(Page::Connect);
	(phone, ops)
}

fn has(labels: &[String], text: &str) -> bool {
	labels.iter().any(|l| l.contains(text))
}

async fn trust(phone: &mut Phone) -> phone::Drawn {
	phone.click_label("Trust this device");
	phone.click_label("Yes, trust it");
	tokio::time::sleep(std::time::Duration::from_millis(100)).await;
	phone.settle()
}

#[tokio::test]
async fn an_unreachable_device_shows_why_and_stays_nearby() {
	let (mut phone, _ops) = rig(Some("UNAVAILABLE"));
	let drawn = trust(&mut phone).await;
	assert!(
		has(&drawn.labels, "could not reach that device"),
		"{:?}",
		drawn.labels
	);
	assert!(
		has(&drawn.labels, "Trust this device"),
		"{:?}",
		drawn.labels
	);
}

#[tokio::test]
async fn a_different_key_is_said_plainly() {
	let (mut phone, _ops) = rig(Some("TRUST_KEY_MISMATCH"));
	let drawn = trust(&mut phone).await;
	assert!(
		has(&drawn.labels, "does not have the key you were shown"),
		"{:?}",
		drawn.labels
	);
	assert!(
		has(&drawn.labels, "Nothing was saved"),
		"{:?}",
		drawn.labels
	);
}

#[tokio::test]
async fn a_stored_decision_moves_the_device_into_contacts() {
	let (mut phone, _ops) = rig(None);
	phone.click_label("Trust this device");
	phone.click_label("Yes, trust it");
	tokio::time::sleep(std::time::Duration::from_millis(100)).await;
	// The engine's next state lists the device as trusted.
	let mut trusted = stranger();
	trusted.trust = Trust::TofuUnverified;
	trusted.reachable = true;
	let mut state = phone::busy_state();
	state.peers.push(trusted);
	phone.show_state(&state);
	let connect = phone.page(Page::Connect);
	assert!(
		!has(&connect.labels, "Trust this device"),
		"{:?}",
		connect.labels
	);
	let contacts = phone.page(Page::Contacts);
	assert!(
		has(&contacts.labels, "Dantes Laptop"),
		"{:?}",
		contacts.labels
	);
	assert!(!has(&contacts.labels, "could not"), "no error on success");
}
