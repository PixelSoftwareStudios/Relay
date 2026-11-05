mod common;

use common::*;
use relay_api::pb::CallState;
use relay_desktop::mock::{meters_at, seed};
use relay_desktop::model::{AppState, CallMeter, Meters, Mode, Phase, Trust};
use relay_desktop::viewmodel::*;

fn state_with(calls: Vec<relay_api::pb::Call>) -> AppState {
	let mut s = AppState {
		client_id: ME.to_vec(),
		..AppState::default()
	};
	s.load_snapshot(&snapshot(1, calls));
	s
}

#[test]
fn cards_show_live_calls_only_with_peer_names() {
	let s = state_with(vec![pb_call(CallState::Ended, None)]);
	assert!(call_cards(&s, &Meters::default()).is_empty());
	let s = state_with(vec![pb_call(CallState::IncomingRinging, None)]);
	let cards = call_cards(&s, &Meters::default());
	assert_eq!(cards[0].name, "Ada");
	assert!(cards[0].incoming && !cards[0].controllable);
}

#[test]
fn speaking_and_level_need_a_connected_call_with_meter_data() {
	let s = state_with(vec![pb_call(CallState::Connected, Some((&ME, 1)))]);
	let meter = CallMeter {
		call_id: hex::encode(CALL),
		speaking: true,
		receive_dbfs: Some(-20.0),
		rtt_ms: None,
	};
	let meters = Meters {
		calls: vec![meter],
		..Meters::default()
	};
	let card = &call_cards(&s, &meters)[0];
	assert!(card.speaking && card.subtitle.contains("speaking"));
	assert_eq!(card.receive_dbfs, Some(-20.0));
	let none = &call_cards(&s, &Meters::default())[0];
	assert!(!none.speaking && none.receive_dbfs.is_none());
	let ringing = state_with(vec![pb_call(CallState::OutgoingRinging, None)]);
	assert!(!call_cards(&ringing, &meters)[0].speaking);
}

#[test]
fn peers_are_callable_only_when_reachable_trusted_and_idle() {
	let mut s = state_with(vec![]);
	assert!(peer_rows(&s)[0].can_call);
	s.peers[0].trust = Trust::KeyChanged;
	assert!(!peer_rows(&s)[0].can_call);
	s.peers[0].trust = Trust::Verified;
	s.peers[0].reachable = false;
	assert!(!peer_rows(&s)[0].can_call);
	let busy = state_with(vec![pb_call(CallState::Connected, None)]);
	assert!(!peer_rows(&busy)[0].can_call);
}

#[test]
fn modes_other_than_listen_only_need_consent() {
	assert!(mode_enabled(Mode::ReceiveOnly, false));
	assert!(!mode_enabled(Mode::OpenMic, false));
	assert!(mode_enabled(Mode::PushToTalk, true));
}

#[test]
fn mock_seed_is_flagged_and_meters_follow_connected_calls() {
	let mut s = seed();
	assert!(s.mock);
	assert!(meters_at(&s, 1.0).calls.is_empty());
	s.calls[0].phase = Phase::Connected;
	assert_eq!(meters_at(&s, 1.0).calls.len(), 1);
	s.settings.as_mut().unwrap().global_muted = true;
	assert_eq!(meters_at(&s, 1.0).microphone_dbfs, None);
}

#[test]
fn profiles_without_an_avatar_hash_resolve_to_the_placeholder() {
	use relay_desktop::avatar::{Avatar, DEFAULT_PNG};

	let mut s = state_with(vec![pb_call(CallState::Connected, None)]);
	assert_eq!(peer_rows(&s)[0].avatar, Avatar::Placeholder);
	assert_eq!(
		call_cards(&s, &Meters::default())[0].avatar,
		Avatar::Placeholder
	);
	assert_eq!(local_avatar(&s), Avatar::Placeholder);

	s.peers[0].has_avatar = true;
	assert_eq!(peer_rows(&s)[0].avatar, Avatar::Set);

	let img = image::load_from_memory(DEFAULT_PNG).expect("embedded placeholder is a PNG");
	assert_eq!((img.width(), img.height()), (256, 256));
}
