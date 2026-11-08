//! The libFuzzer targets for frame, advert, invite, certificate and session-message parsing, run as
//! plain property loops so every normal test run exercises the same entry points.

use std::net::IpAddr;

use proptest::prelude::*;
use prost::Message;
use relay_net::AddressPolicy;
use relay_net::channel::{Delegation, InviteToken, MembershipCert, Revocation};
use relay_net::control::session_msg::validate;
use relay_net::discovery::{PlatformRecord, parse_platform_record};
use relay_net::proto::SessionMessage;
use relay_net::transfer::fuzzing::{decode_frame, encode_frame};

/// Bytes that start like a real frame, so the interesting branches are reached.
fn framed() -> impl Strategy<Value = Vec<u8>> {
	(
		prop_oneof![Just(1u8), Just(2), Just(3), Just(4), Just(5), any::<u8>()],
		proptest::collection::vec(any::<u8>(), 0..64),
	)
		.prop_map(|(tag, rest)| std::iter::once(tag).chain(rest).collect())
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(1000))]

	#[test]
	fn transfer_frames_are_canonical(data in prop_oneof![framed(), proptest::collection::vec(any::<u8>(), 0..64)]) {
		if let Ok(message) = decode_frame(&data) {
			prop_assert_eq!(encode_frame(&message).unwrap(), data);
		}
	}

	#[test]
	fn invites_and_certificates_never_panic(data in proptest::collection::vec(any::<u8>(), 0..400)) {
		if let Ok(text) = std::str::from_utf8(&data) {
			let _ = InviteToken::parse(text);
		}
		let _ = InviteToken::parse(&format!("RLYI1{}", String::from_utf8_lossy(&data)));
		if let Ok(cert) = MembershipCert::from_bytes(&data) {
			prop_assert_eq!(cert.to_bytes(), data.clone());
		}
		if let Ok(revocation) = Revocation::from_bytes(&data) {
			prop_assert_eq!(revocation.to_bytes(), data.clone());
		}
		let _ = Delegation::from_bytes(&data);
	}

	#[test]
	fn session_messages_validate_without_panicking(data in proptest::collection::vec(any::<u8>(), 0..600)) {
		if let Ok(message) = SessionMessage::decode(data.as_slice()) {
			let _ = validate(&message);
		}
	}

	#[test]
	fn adverts_with_arbitrary_text_never_panic(
		name in "\\PC{0,80}",
		port in any::<u16>(),
		version in prop_oneof![Just("1".to_owned()), "\\PC{0,4}"],
		id in prop_oneof![Just("00112233445566778899aabbccddeeff".to_owned()), "\\PC{0,40}"],
		display in "\\PC{0,300}",
		extra in proptest::collection::vec(("\\PC{0,8}", "\\PC{0,40}"), 0..6),
		addresses in proptest::collection::vec(any::<[u8; 4]>(), 0..4),
	) {
		let mut txt = vec![("v".to_owned(), version), ("id".to_owned(), id), ("n".to_owned(), display)];
		txt.extend(extra);
		let record = PlatformRecord {
			fullname: name,
			port,
			addresses: addresses.into_iter().map(IpAddr::from).collect(),
			txt,
		};
		for policy in [AddressPolicy::lan_only(), AddressPolicy::lan_and_loopback()] {
			if let Some(peer) = parse_platform_record(&record, &policy) {
				prop_assert!(peer.display_name.len() <= 64 * 4);
			}
		}
	}
}
