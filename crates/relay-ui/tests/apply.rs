mod common;

use common::*;
use relay_api::pb::{self, state_event::Event};
use relay_ui::model::{AppState, Direction, Phase, Trust};

fn loaded() -> AppState {
	let mut s = AppState {
		client_id: ME.to_vec(),
		..AppState::default()
	};
	s.load_snapshot(&snapshot(
		3,
		vec![pb_call(pb::CallState::Connected, Some((&ME, 2)))],
	));
	s
}

fn apply(s: &mut AppState, revision: u64, event: Event) {
	s.apply_event(revision, &pb::StateEvent { event: Some(event) });
}

#[test]
fn snapshot_converts_ids_enums_and_ownership() {
	let s = loaded();
	assert_eq!(s.revision, 3);
	assert_eq!(s.peers[0].peer_id, hex::encode(PEER));
	assert_eq!(s.peers[0].trust, Trust::Verified);
	assert_eq!(s.calls[0].phase, Phase::Connected);
	assert!(s.calls[0].controlled_by_me);
	assert_eq!(s.calls[0].controller_generation, 2);
	assert_eq!(s.devices[2].direction, Direction::Output);
}

#[test]
fn unknown_enum_values_degrade_conservatively() {
	let mut s = loaded();
	let mut call = pb_call(pb::CallState::Connected, None);
	call.state = 999;
	call.transmit = Some(pb::TransmitRoute {
		mode: 999,
		effective_transmitting: false,
		..Default::default()
	});
	apply(&mut s, 4, Event::Call(call));
	assert_eq!(s.calls[0].phase, Phase::Preparing);
	assert_eq!(s.calls[0].transmit.mode, relay_ui::model::Mode::Muted);
	assert!(!s.calls[0].controlled_by_me);
}

#[test]
fn events_upsert_and_remove_by_kind() {
	let mut s = loaded();
	apply(
		&mut s,
		4,
		Event::Peer(pb::Peer {
			display_name: "Renamed".into(),
			..pb_peer("x")
		}),
	);
	assert_eq!(s.peers.len(), 1);
	assert_eq!(s.peers[0].display_name, "Renamed");
	apply(
		&mut s,
		5,
		Event::Removed(pb::Removed {
			id: CALL.to_vec(),
			kind: "call".into(),
		}),
	);
	assert!(s.calls.is_empty());
	apply(
		&mut s,
		6,
		Event::Removed(pb::Removed {
			id: b"mic".to_vec(),
			kind: "device".into(),
		}),
	);
	assert_eq!(s.devices.len(), 2);
	apply(
		&mut s,
		7,
		Event::Removed(pb::Removed {
			id: vec![1],
			kind: "mystery".into(),
		}),
	);
	assert_eq!(s.revision, 7);
}

#[test]
fn collections_and_warnings_stay_bounded() {
	let mut s = AppState::default();
	for i in 0..400u32 {
		let mut p = pb_peer("p");
		p.peer_id = [i.to_be_bytes().to_vec(), vec![0; 28]].concat();
		apply(&mut s, u64::from(i), Event::Peer(p));
	}
	assert_eq!(s.peers.len(), 256);
	for i in 0..40 {
		apply(
			&mut s,
			1000 + i,
			Event::Warning(pb::Problem {
				code: format!("w{i}"),
				..Default::default()
			}),
		);
	}
	assert_eq!(s.warnings.len(), 16);
	apply(
		&mut s,
		2000,
		Event::Warning(pb::Problem {
			code: "w39".into(),
			..Default::default()
		}),
	);
	assert_eq!(s.warnings.len(), 16, "duplicate warnings are not repeated");
}

#[test]
fn going_down_drops_engine_data_so_stale_state_is_not_shown() {
	let mut s = loaded();
	s.set_link_down(relay_ui::model::Link::Reconnecting, Some("gone".into()));
	assert!(s.calls.is_empty() && s.peers.is_empty() && s.devices.is_empty());
	assert_eq!(s.link_error.as_deref(), Some("gone"));
}

#[test]
fn state_serializes_camel_case_without_internal_fields() {
	let json = serde_json::to_value(loaded()).unwrap();
	assert_eq!(json["link"], "connecting");
	assert_eq!(json["calls"][0]["phase"], "connected");
	assert_eq!(json["calls"][0]["controlledByMe"], true);
	assert!(json["calls"][0].get("controllerGeneration").is_none());
	assert!(json.get("clientId").is_none());
}

#[test]
fn a_withdrawn_warning_disappears() {
	let mut s = AppState::default();
	for code in ["MICROPHONE_UNAVAILABLE", "OTHER"] {
		apply(
			&mut s,
			1,
			Event::Warning(pb::Problem {
				code: code.into(),
				..Default::default()
			}),
		);
	}
	apply(
		&mut s,
		2,
		Event::Removed(pb::Removed {
			id: b"MICROPHONE_UNAVAILABLE".to_vec(),
			kind: "warning".into(),
		}),
	);
	let codes: Vec<&str> = s.warnings.iter().map(|w| w.code.as_str()).collect();
	assert_eq!(codes, ["OTHER"]);
}
