mod channel_support;

use channel_support::*;
use relay_net::channel::{
	AdmissionService, Begin, ChannelStore, Credential, InviteToken, JoinClient, JoinError, Policy,
	SessionBinding, limits,
};
use relay_net::identity::Identity;
use relay_net::proto::channel_message::Body;
use relay_net::session::LinkEpoch;

const PASSWORD_ONLY: Policy = Policy {
	listed: false,
	password: true,
	invite_only: false,
};
const INVITE_ONLY: Policy = Policy {
	listed: false,
	password: false,
	invite_only: true,
};

fn owner() -> Identity {
	Identity::generate().unwrap()
}

#[test]
fn open_channel_join_yields_a_verifying_certificate() {
	let pair = Pair::new();
	let w = world(OPEN, None, &owner().fingerprint());
	let record = run_join(&pair, &w, &Credential::open(&w.channel), None, mono(1)).unwrap();
	assert_eq!(record.id, w.channel);
	assert_eq!(record.name.as_str(), "Floor 3");
	let cert = record.cert.unwrap();
	cert.verify(&record.owner_public, &pair.joiner_fp(), NOW)
		.unwrap();
}

#[test]
fn password_join_succeeds_with_the_right_password_only() {
	let pair = Pair::new();
	let w = world(PASSWORD_ONLY, Some("correct horse"), &owner().fingerprint());
	let right = Credential::from_password("correct horse", &w.channel).unwrap();
	let wrong = Credential::from_password("correct hOrse", &w.channel).unwrap();
	assert_eq!(
		run_join(&pair, &w, &wrong, None, mono(1)).unwrap_err(),
		JoinError::Denied
	);
	assert!(run_join(&pair, &w, &right, None, mono(2)).is_ok());
}

#[test]
fn password_bounds_are_enforced() {
	let id = relay_net::channel::ChannelId::from_bytes([1; 16]);
	assert!(Credential::from_password("short", &id).is_err());
	assert!(Credential::from_password(&"x".repeat(129), &id).is_err());
}

#[test]
fn guessing_locks_the_peer_out_even_when_it_never_confirms() {
	let pair = Pair::new();
	let w = world(PASSWORD_ONLY, Some("correct horse"), &owner().fingerprint());
	let wrong = Credential::from_bytes([9; 32]);
	let right = Credential::from_password("correct horse", &w.channel).unwrap();
	// Each wrong guess is charged at `begin`, although the client aborts on seeing the reply.
	for i in 0..limits::PEER_CHANNEL_FAILURES {
		assert!(run_join(&pair, &w, &wrong, None, mono(u64::from(i))).is_err());
	}
	// Now even the right password is refused, without a PAKE reply.
	let (_, start) = JoinClient::start(&pair.joiner_binding, w.channel, &right, None);
	assert!(matches!(
		w.service.begin(
			&pair.admitter_binding,
			&pair.joiner_fp(),
			&start,
			mono(10),
			NOW
		),
		Begin::Denied(_)
	));
	// After the lockout (60 s) it works again.
	assert!(run_join(&pair, &w, &right, None, mono(10 + 61)).is_ok());
}

#[test]
fn many_identities_hit_the_channel_wide_budget() {
	let w = world(PASSWORD_ONLY, Some("correct horse"), &owner().fingerprint());
	let wrong = Credential::from_bytes([9; 32]);
	for i in 0..limits::CHANNEL_FAILURES {
		let pair = Pair::new();
		assert!(run_join(&pair, &w, &wrong, None, mono(i as u64)).is_err());
	}
	// A fresh identity with the right password is now locked out by the channel budget.
	let pair = Pair::new();
	let right = Credential::from_password("correct horse", &w.channel).unwrap();
	assert!(run_join(&pair, &w, &right, None, mono(30)).is_err());
	assert!(run_join(&pair, &w, &right, None, mono(30 + 901)).is_ok());
}

#[test]
fn unknown_channel_is_indistinguishable_from_wrong_password() {
	let pair = Pair::new();
	let w = world(PASSWORD_ONLY, Some("correct horse"), &owner().fingerprint());
	let bogus = relay_net::channel::ChannelId::from_bytes([7; 16]);
	let (mut client, start) = JoinClient::start(
		&pair.joiner_binding,
		bogus,
		&Credential::from_bytes([1; 32]),
		None,
	);
	// The admitter still answers with a full-looking reply.
	let Begin::Reply(session, reply) = w.service.begin(
		&pair.admitter_binding,
		&pair.joiner_fp(),
		&start,
		mono(1),
		NOW,
	) else {
		panic!("unknown channel must run a full exchange");
	};
	assert!(matches!(reply.body, Some(Body::JoinReply(_))));
	assert_eq!(client.on_reply(&reply).unwrap_err(), JoinError::Denied);
	drop(session);
}

#[test]
fn exchange_cannot_be_replayed_on_another_session() {
	let a = Pair::new();
	let w = world(OPEN, None, &owner().fingerprint());
	let cred = Credential::open(&w.channel);
	let (mut client, start) = JoinClient::start(&a.joiner_binding, w.channel, &cred, None);
	let Begin::Reply(_session, reply) =
		w.service
			.begin(&a.admitter_binding, &a.joiner_fp(), &start, mono(1), NOW)
	else {
		panic!("reply expected");
	};
	// The same recorded start, offered on a different link (fresh epoch, other keys): the
	// admitter's reference check cannot match, so it is treated as an unknown channel and the
	// confirmation the joiner computed for link A fails.
	let b = Pair::new();
	let Begin::Reply(session_b, _) =
		w.service
			.begin(&b.admitter_binding, &b.joiner_fp(), &start, mono(2), NOW)
	else {
		panic!("a full exchange is run either way");
	};
	let confirm = client.on_reply(&reply).unwrap();
	let outcome = session_b.on_confirm(&confirm, NOW);
	assert!(matches!(outcome.body, Some(Body::JoinDenied(_))));
}

#[test]
fn same_epoch_with_different_keys_does_not_bind() {
	let epoch = LinkEpoch::random();
	let a = Pair::with(
		Identity::generate().unwrap(),
		Identity::generate().unwrap(),
		epoch,
	);
	let b = Pair::with(
		Identity::generate().unwrap(),
		Identity::generate().unwrap(),
		epoch,
	);
	assert_ne!(a.joiner_binding.digest(), b.joiner_binding.digest());
	let swapped = SessionBinding::new(&epoch, a.admitter.public(), a.joiner.public(), true);
	assert_ne!(a.joiner_binding.digest(), swapped.digest());
}

#[test]
fn invite_is_single_use_and_expires() {
	let owner_id = owner();
	let w = world(INVITE_ONLY, None, &owner_id.fingerprint());
	let mut rec = w.store.get(&w.channel).unwrap().unwrap();
	let token = rec
		.new_invite(&owner_id.fingerprint(), NOW, 600, 1)
		.unwrap();
	w.store.put(rec.clone()).unwrap();
	let text = token.encode();
	let parsed = InviteToken::parse(&text).unwrap();

	let first = Pair::new();
	let ok = run_join(&first, &w, &parsed.credential(), Some(parsed.id), mono(1));
	assert!(ok.is_ok());

	// Spent: a second joiner holding the same token is denied.
	let second = Pair::new();
	assert!(run_join(&second, &w, &parsed.credential(), Some(parsed.id), mono(2)).is_err());

	// Expired token.
	let mut rec = w.store.get(&w.channel).unwrap().unwrap();
	let late = rec
		.new_invite(&owner_id.fingerprint(), NOW - 1000, 600, 1)
		.unwrap();
	w.store.put(rec).unwrap();
	let third = Pair::new();
	assert!(run_join(&third, &w, &late.credential(), Some(late.id), mono(3)).is_err());
}

#[test]
fn forged_invite_and_missing_invite_fail() {
	let owner_id = owner();
	let w = world(INVITE_ONLY, None, &owner_id.fingerprint());
	let mut rec = w.store.get(&w.channel).unwrap().unwrap();
	let token = rec
		.new_invite(&owner_id.fingerprint(), NOW, 600, 1)
		.unwrap();
	w.store.put(rec).unwrap();
	let pair = Pair::new();
	// Right id, wrong secret.
	let forged = Credential::from_invite_secret(&[3; 32], &token.id, &w.channel);
	assert!(run_join(&pair, &w, &forged, Some(token.id), mono(1)).is_err());
	// Open credential and no invite on an invite-only channel.
	assert!(run_join(&pair, &w, &Credential::open(&w.channel), None, mono(2)).is_err());
	// The real token still works afterwards (failures did not consume it).
	assert!(run_join(&pair, &w, &token.credential(), Some(token.id), mono(3)).is_ok());
}

#[test]
fn multi_use_invite_counts_down() {
	let owner_id = owner();
	let w = world(INVITE_ONLY, None, &owner_id.fingerprint());
	let mut rec = w.store.get(&w.channel).unwrap().unwrap();
	let token = rec
		.new_invite(&owner_id.fingerprint(), NOW, 600, 2)
		.unwrap();
	w.store.put(rec).unwrap();
	for i in 0..2 {
		let pair = Pair::new();
		assert!(run_join(&pair, &w, &token.credential(), Some(token.id), mono(i)).is_ok());
	}
	let pair = Pair::new();
	assert!(run_join(&pair, &w, &token.credential(), Some(token.id), mono(9)).is_err());
}

#[test]
fn password_plus_invite_requires_both() {
	let owner_id = owner();
	let policy = Policy {
		listed: false,
		password: true,
		invite_only: true,
	};
	let w = world(policy, Some("correct horse"), &owner_id.fingerprint());
	let mut rec = w.store.get(&w.channel).unwrap().unwrap();
	let token = rec
		.new_invite(&owner_id.fingerprint(), NOW, 600, 3)
		.unwrap();
	w.store.put(rec).unwrap();
	let pw = Credential::from_password("correct horse", &w.channel).unwrap();
	let pair = Pair::new();
	assert!(run_join(&pair, &w, &pw, Some(token.id), mono(1)).is_err());
	assert!(run_join(&pair, &w, &token.credential(), Some(token.id), mono(2)).is_err());
	let both = pw.combine(&token.credential());
	assert!(run_join(&pair, &w, &both, Some(token.id), mono(3)).is_ok());
}

#[test]
fn grant_for_another_owner_key_is_rejected() {
	let pair = Pair::new();
	let honest = world(OPEN, None, &owner().fingerprint());
	let rogue = world(OPEN, None, &owner().fingerprint());
	// The joiner asked for the honest channel but a rogue admitter holds a different one with
	// a matching reference only if it knew the id, which it does not: it is denied outright.
	let (mut client, start) = JoinClient::start(
		&pair.joiner_binding,
		honest.channel,
		&Credential::open(&honest.channel),
		None,
	);
	let Begin::Reply(session, reply) = rogue.service.begin(
		&pair.admitter_binding,
		&pair.joiner_fp(),
		&start,
		mono(1),
		NOW,
	) else {
		panic!("reply expected");
	};
	assert_eq!(client.on_reply(&reply).unwrap_err(), JoinError::Denied);
	drop(session);
}

#[test]
fn concurrent_exchanges_are_bounded() {
	let w = world(OPEN, None, &owner().fingerprint());
	let cred = Credential::open(&w.channel);
	let mut held = Vec::new();
	for _ in 0..limits::JOINS_CONCURRENT_MAX {
		let pair = Pair::new();
		let (_, start) = JoinClient::start(&pair.joiner_binding, w.channel, &cred, None);
		match w.service.begin(
			&pair.admitter_binding,
			&pair.joiner_fp(),
			&start,
			mono(0),
			NOW,
		) {
			Begin::Reply(s, _) => held.push(s),
			Begin::Denied(_) => panic!("under the bound"),
		}
	}
	let pair = Pair::new();
	let (_, start) = JoinClient::start(&pair.joiner_binding, w.channel, &cred, None);
	assert!(matches!(
		w.service.begin(
			&pair.admitter_binding,
			&pair.joiner_fp(),
			&start,
			mono(0),
			NOW
		),
		Begin::Denied(_)
	));
	held.pop();
	let (_, start) = JoinClient::start(&pair.joiner_binding, w.channel, &cred, None);
	assert!(matches!(
		w.service.begin(
			&pair.admitter_binding,
			&pair.joiner_fp(),
			&start,
			mono(0),
			NOW
		),
		Begin::Reply(..)
	));
}

#[test]
fn service_type_is_cloneable_for_many_links() {
	let w = world(OPEN, None, &owner().fingerprint());
	let _second: AdmissionService = w.service.clone();
}
