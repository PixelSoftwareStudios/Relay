//! Target resolution: peers, calls and devices from user input.

mod common;

use common::fixtures::{self, ALICE, BOB};
use relay_api::pb::{DeviceDirection, Peer};
use relay_cli::cli::TargetError;
use relay_cli::commands::resolve::*;
use relay_cli::commands::trust::parse_fingerprint;

#[test]
fn peer_by_name_any_case_prefix_and_id() {
	let s = fixtures::snapshot();
	assert_eq!(resolve_peer(&s, "alice").unwrap().peer_id, ALICE);
	assert_eq!(resolve_peer(&s, "BO").unwrap().peer_id, BOB);
	assert_eq!(resolve_peer(&s, "aaaa").unwrap().peer_id, ALICE);
	assert!(matches!(
		resolve_peer(&s, "carol"),
		Err(TargetError::NotFound { .. })
	));
}

#[test]
fn ambiguous_prefix_lists_candidates_instead_of_guessing() {
	let mut s = fixtures::snapshot();
	s.peers.push(Peer {
		peer_id: vec![0xab; 32],
		display_name: "Alicia".into(),
		..Peer::default()
	});
	let err = resolve_peer(&s, "ali").unwrap_err();
	match err {
		TargetError::Ambiguous { candidates, .. } => {
			assert!(candidates.contains("Alice") && candidates.contains("Alicia"));
		}
		other => panic!("{other:?}"),
	}
	// An exact name still wins over the longer prefix match.
	assert_eq!(resolve_peer(&s, "alice").unwrap().peer_id, ALICE);
}

#[test]
fn call_defaults_to_the_single_live_call_and_ignores_ended_ones() {
	let mut s = fixtures::snapshot();
	assert_eq!(resolve_call(&s, None).unwrap().call_id, fixtures::CALL);
	assert_eq!(
		resolve_call(&s, Some("c1c1")).unwrap().call_id,
		fixtures::CALL
	);
	assert_eq!(
		resolve_call(&s, Some("Alice")).unwrap().call_id,
		fixtures::CALL
	);

	let mut second = s.calls[0].clone();
	second.call_id = vec![0xd2; 16];
	s.calls.push(second);
	assert!(matches!(
		resolve_call(&s, None),
		Err(TargetError::NoDefault(_))
	));

	s.calls[0].state = relay_api::pb::CallState::Ended as i32;
	s.calls[1].state = relay_api::pb::CallState::Ended as i32;
	assert!(matches!(
		resolve_call(&s, None),
		Err(TargetError::NoDefault(_))
	));
}

#[test]
fn devices_are_matched_within_their_direction() {
	let s = fixtures::snapshot();
	assert_eq!(
		resolve_device(&s, "usb", DeviceDirection::Input)
			.unwrap()
			.device_id,
		"mic2"
	);
	assert_eq!(
		resolve_device(&s, "spk1", DeviceDirection::Output)
			.unwrap()
			.name,
		"Speakers"
	);
	assert!(resolve_device(&s, "spk1", DeviceDirection::Input).is_err());
}

#[test]
fn manual_addresses_are_recognised() {
	assert_eq!(
		parse_address("192.168.1.9:7000").as_deref(),
		Some("192.168.1.9:7000")
	);
	assert!(parse_address("alice").is_none());
	assert!(parse_address("192.168.1.9").is_none());
}

#[test]
fn fingerprints_accept_separators_and_reject_wrong_lengths() {
	let hex64 = "0f".repeat(32);
	assert_eq!(parse_fingerprint(&hex64).unwrap().len(), 32);
	let colons = hex64
		.as_bytes()
		.chunks(2)
		.map(|c| std::str::from_utf8(c).unwrap())
		.collect::<Vec<_>>()
		.join(":");
	assert_eq!(parse_fingerprint(&colons).unwrap().len(), 32);
	assert!(parse_fingerprint("0f0f").is_err());
	assert!(parse_fingerprint(&"zz".repeat(32)).is_err());
}
