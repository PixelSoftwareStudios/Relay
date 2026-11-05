mod channel_support;

use channel_support::*;
use proptest::prelude::*;
use prost::Message;
use relay_net::channel::cert::{MembershipCert, Revocation};
use relay_net::channel::tag::ChannelAdvert;
use relay_net::channel::{
	AdmissionService, Begin, ChannelStore, InMemoryChannelStore, InviteToken, check_proof, wire,
};
use relay_net::identity::Identity;
use relay_net::proto::{ChannelMessage, ChannelProof};
use std::sync::Arc;

proptest! {
	#![proptest_config(ProptestConfig::with_cases(512))]

	#[test]
	fn decode_never_panics_and_respects_the_bound(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
		let result = wire::decode(&bytes);
		if bytes.len() > relay_net::channel::limits::MESSAGE_MAX_BYTES {
			prop_assert!(result.is_err());
		}
	}

	#[test]
	fn certificate_and_revocation_parsers_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
		let _ = MembershipCert::from_bytes(&bytes);
		let _ = Revocation::from_bytes(&bytes);
	}

	#[test]
	fn invite_parser_never_panics(text in ".{0,200}") {
		let _ = InviteToken::parse(&text);
	}

	#[test]
	fn txt_parser_never_panics(epoch in ".{0,24}", tags in ".{0,200}", listed in proptest::collection::vec(".{0,300}", 0..6)) {
		let refs: Vec<&str> = listed.iter().map(String::as_str).collect();
		let _ = ChannelAdvert::from_txt(Some(&epoch), Some(&tags), &refs);
	}

	#[test]
	fn arbitrary_proofs_never_prove_anything(
		channel_ref in proptest::collection::vec(any::<u8>(), 0..32),
		cert in proptest::collection::vec(any::<u8>(), 0..300),
		revocations in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..300), 0..20),
	) {
		let pair = Pair::new();
		let owner = Identity::generate().unwrap();
		let w = world(OPEN, None, &owner.fingerprint());
		let record = w.store.get(&w.channel).unwrap().unwrap();
		let message = ChannelMessage {
			body: Some(relay_net::proto::channel_message::Body::Proof(ChannelProof {
				channel_ref, cert, revocations, is_reply: false,
			})),
		};
		let out = check_proof(std::slice::from_ref(&record), &pair.admitter_binding, &pair.joiner_fp(), &message, NOW);
		prop_assert!(out.proven.is_none());
	}

	#[test]
	fn admitter_survives_arbitrary_join_messages(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
		let pair = Pair::new();
		let store = Arc::new(InMemoryChannelStore::new());
		let service = AdmissionService::new(store as Arc<dyn ChannelStore>);
		if let Ok(message) = ChannelMessage::decode(bytes.as_slice()) {
			let _ = wire::validate(&message);
			let begin = service.begin(&pair.admitter_binding, &pair.joiner_fp(), &message, mono(1), NOW);
			prop_assert!(matches!(begin, Begin::Denied(_)) || matches!(begin, Begin::Reply(..)));
		}
	}
}

#[test]
fn invite_tokens_roundtrip_and_reject_damage() {
	let issuer = Identity::generate().unwrap().fingerprint();
	let (token, _) = relay_net::channel::invite::create(
		relay_net::channel::ChannelId::from_bytes([5; 16]),
		&issuer,
		NOW,
		600,
		1,
	)
	.unwrap();
	let text = token.encode();
	assert!(text.starts_with("RLYI1"));
	let back = InviteToken::parse(&text).unwrap();
	assert_eq!(back.id, token.id);
	assert_eq!(
		*back.credential().to_bytes(),
		*token.credential().to_bytes()
	);
	// Case, spaces and hyphens are forgiven.
	let spaced: String = text
		.to_lowercase()
		.chars()
		.collect::<Vec<_>>()
		.chunks(6)
		.map(|c| c.iter().collect::<String>())
		.collect::<Vec<_>>()
		.join("-");
	assert!(InviteToken::parse(&spaced).is_ok());
	// Any single changed character is caught by the checksum or the canonical form.
	let bytes = text.as_bytes();
	for i in 5..bytes.len() {
		let mut bad = bytes.to_vec();
		bad[i] = if bad[i] == b'A' { b'B' } else { b'A' };
		assert!(
			InviteToken::parse(std::str::from_utf8(&bad).unwrap()).is_err(),
			"position {i}"
		);
	}
	assert!(InviteToken::parse(&text[..text.len() - 1]).is_err());
	assert!(InviteToken::parse("").is_err());
}

#[test]
fn invite_parameters_are_range_checked() {
	let issuer = Identity::generate().unwrap().fingerprint();
	let id = relay_net::channel::ChannelId::from_bytes([5; 16]);
	use relay_net::channel::invite::create;
	assert!(create(id, &issuer, NOW, 0, 1).is_err());
	assert!(create(id, &issuer, NOW, 8 * 86_400, 1).is_err());
	assert!(create(id, &issuer, NOW, 60, 0).is_err());
	assert!(create(id, &issuer, NOW, 60, 9).is_err());
}

#[test]
fn active_invites_are_bounded() {
	let owner = Identity::generate().unwrap();
	let policy = relay_net::channel::Policy {
		listed: false,
		password: false,
		invite_only: true,
	};
	let mut record =
		relay_net::channel::new_channel("Club", policy, None, &owner.fingerprint(), NOW).unwrap();
	for _ in 0..relay_net::channel::limits::INVITES_ACTIVE_MAX {
		record.new_invite(&owner.fingerprint(), NOW, 60, 1).unwrap();
	}
	assert_eq!(
		record
			.new_invite(&owner.fingerprint(), NOW, 60, 1)
			.unwrap_err(),
		relay_net::channel::InviteError::TooMany
	);
	// Expired invites free their slots.
	assert!(
		record
			.new_invite(&owner.fingerprint(), NOW + 61, 60, 1)
			.is_ok()
	);
}

#[test]
fn issued_members_are_bounded() {
	let owner = Identity::generate().unwrap();
	let mut record =
		relay_net::channel::new_channel("Club", OPEN, None, &owner.fingerprint(), NOW).unwrap();
	for _ in 0..relay_net::channel::limits::MEMBERS_ISSUED_MAX {
		let fp = Identity::generate().unwrap().fingerprint();
		assert!(record.admit(&fp, NOW).is_some());
	}
	assert!(
		record
			.admit(&Identity::generate().unwrap().fingerprint(), NOW)
			.is_none()
	);
}

#[test]
fn store_bounds_channels_and_wipes_on_remove() {
	let owner = Identity::generate().unwrap();
	let store = InMemoryChannelStore::new();
	for i in 0..relay_net::channel::limits::CHANNELS_MAX {
		let r = relay_net::channel::new_channel(
			&format!("c{i}"),
			OPEN,
			None,
			&owner.fingerprint(),
			NOW,
		)
		.unwrap();
		store.put(r).unwrap();
	}
	let extra =
		relay_net::channel::new_channel("extra", OPEN, None, &owner.fingerprint(), NOW).unwrap();
	assert_eq!(
		store.put(extra.clone()).unwrap_err(),
		relay_net::channel::StoreError::Full
	);
	let first = store.list().unwrap()[0].id;
	assert!(store.remove(&first).unwrap());
	assert!(store.get(&first).unwrap().is_none());
	store.put(extra).unwrap();
}

#[test]
fn creating_a_channel_validates_name_and_password() {
	let owner = Identity::generate().unwrap().fingerprint();
	use relay_net::channel::{CreateError, Policy, new_channel};
	let pw = Policy {
		listed: false,
		password: true,
		invite_only: false,
	};
	assert!(matches!(
		new_channel("", OPEN, None, &owner, NOW),
		Err(CreateError::Name(_))
	));
	assert!(matches!(
		new_channel("ok", pw, None, &owner, NOW),
		Err(CreateError::PasswordMismatch)
	));
	assert!(matches!(
		new_channel("ok", OPEN, Some("whatever1"), &owner, NOW),
		Err(CreateError::PasswordMismatch)
	));
	assert!(matches!(
		new_channel("ok", pw, Some("short"), &owner, NOW),
		Err(CreateError::Password(_))
	));
}

#[test]
fn debug_output_never_contains_secrets() {
	let owner = Identity::generate().unwrap();
	let record =
		relay_net::channel::new_channel("Club", OPEN, None, &owner.fingerprint(), NOW).unwrap();
	let text = format!("{record:?} {:?}", record.authority);
	let seed = record
		.authority
		.as_ref()
		.unwrap()
		.seed()
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect::<String>();
	assert!(!text.contains(&seed));
	assert!(!text.contains(&record.id.short().repeat(2)));
}
