//! A nearby device can be called without trusting it first; an incoming call from one
//! says honestly that it is not a contact; Block and Unblock reach the engine.

mod common;
mod phone;

use std::sync::Arc;

use common::FakeOps;
use phone::Phone;
use relay_ui::capabilities::SettingsCategory;
use relay_ui::engine::Command;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::{AppState, Peer, Phase, Trust};

fn stranger(trust: Trust, reachable: bool) -> Peer {
	Peer {
		peer_id: "ab".repeat(32),
		display_name: "Dantes Laptop".into(),
		addresses: vec!["192.168.1.77:7000".into()],
		trust,
		reachable,
		avatar_hash: String::new(),
		fingerprint_short: "ABAB-ABAB-ABAB-ABAB-ABAB".into(),
		shared_channels: Vec::new(),
		favorite: false,
		favorite_position: 0,
		key_changed: false,
		previous_peer_id: String::new(),
		previous_fingerprint_short: String::new(),
		auto_answer: false,
		port: 7000,
		last_seen_unix_ms: 0,
	}
}

fn rig(state: AppState) -> (Phone, Arc<FakeOps>) {
	let phone = Phone::desktop(720.0, 2400.0, Language::En);
	let ops = Arc::new(FakeOps::default());
	phone.slot.set(ops.clone());
	phone.show_state(&state);
	phone.actions.hub.update(|s| *s = state.clone());
	(phone, ops)
}

fn has(labels: &[String], text: &str) -> bool {
	labels.iter().any(|l| l.contains(text))
}

async fn settle_ops() {
	tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn a_nearby_device_is_called_without_trusting_it() {
	let mut state = phone::busy_state();
	state.peers.push(stranger(Trust::Unknown, true));
	let (mut phone, ops) = rig(state);
	let drawn = phone.page(Page::Connect);
	assert!(
		has(&drawn.labels, "Trust this device"),
		"the careful option stays"
	);
	assert!(
		has(&drawn.labels, "adds it to Contacts as unverified"),
		"{:?}",
		drawn.labels
	);
	phone.click_label("Call");
	settle_ops().await;
	assert_eq!(
		ops.recorded(),
		vec![Command::StartCall {
			peer_id: vec![0xab; 32]
		}]
	);
	assert_eq!(phone.app.page(), Page::Calls);
}

#[tokio::test]
async fn an_unreachable_nearby_device_cannot_be_called() {
	let mut state = phone::busy_state();
	state.peers.push(stranger(Trust::Unknown, false));
	let (mut phone, ops) = rig(state);
	phone.page(Page::Connect);
	phone.click_label("Call");
	settle_ops().await;
	assert!(ops.recorded().is_empty());
}

fn ringing_from(trust: Trust) -> AppState {
	let mut state = phone::busy_state();
	state.peers.push(stranger(trust, true));
	let mut call = state.calls[0].clone();
	call.call_id = "07".repeat(16);
	call.peer_id = "ab".repeat(32);
	call.phase = Phase::IncomingRinging;
	call.controlled_by_me = true;
	state.calls = vec![call];
	state
}

#[tokio::test]
async fn an_incoming_call_from_a_stranger_says_so_and_can_be_blocked() {
	let (mut phone, ops) = rig(ringing_from(Trust::Unknown));
	let drawn = phone.page(Page::Calls);
	assert!(
		has(&drawn.labels, "Not in your contacts yet"),
		"{:?}",
		drawn.labels
	);
	assert!(
		has(&drawn.labels, "ABAB-ABAB-ABAB-ABAB-ABAB"),
		"{:?}",
		drawn.labels
	);
	phone.click_label("Block");
	settle_ops().await;
	let sent = ops.recorded();
	assert!(
		sent.contains(&Command::BlockPeer {
			peer_id: vec![0xab; 32]
		}),
		"{sent:?}"
	);
}

#[tokio::test]
async fn a_known_caller_is_not_called_a_stranger() {
	let (mut phone, _ops) = rig(ringing_from(Trust::TofuUnverified));
	let drawn = phone.page(Page::Calls);
	assert!(
		!has(&drawn.labels, "Not in your contacts yet"),
		"{:?}",
		drawn.labels
	);
}

#[tokio::test]
async fn blocked_devices_are_listed_in_settings_and_unblock_forgets_the_entry() {
	let mut state = phone::busy_state();
	state.peers.push(stranger(Trust::Blocked, false));
	let (mut phone, ops) = rig(state);
	let connect = phone.page(Page::Connect);
	assert!(
		!has(&connect.labels, "Dantes Laptop"),
		"not under Nearby: {:?}",
		connect.labels
	);
	phone.app.open_settings(Some(SettingsCategory::Privacy));
	let drawn = phone.settle();
	assert!(has(&drawn.labels, "Blocked devices"), "{:?}", drawn.labels);
	assert!(has(&drawn.labels, "Dantes Laptop"), "{:?}", drawn.labels);
	phone.click_label("Unblock");
	settle_ops().await;
	assert_eq!(
		ops.recorded(),
		vec![Command::ForgetPeer {
			peer_id: vec![0xab; 32]
		}]
	);
}

#[tokio::test]
async fn a_blocked_device_is_not_among_contacts_either() {
	let mut state = phone::busy_state();
	state.peers.push(stranger(Trust::Blocked, false));
	let (mut phone, _ops) = rig(state);
	let contacts = phone.page(Page::Contacts);
	assert!(
		!has(&contacts.labels, "Dantes Laptop"),
		"{:?}",
		contacts.labels
	);
}
