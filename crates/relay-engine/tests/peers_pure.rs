use std::time::Duration;

use relay_core::{CallId, PeerEvent, PeerMessage, SessionId};
use relay_engine::peers::{
	BackoffConfig, Inbound, LinkFacts, SessionTable, Verdict, decode, derived_session, elect,
	encode,
};
use relay_net::AddressPolicy;
use relay_net::identity::Fingerprint;

fn fp(byte: u8) -> Fingerprint {
	Fingerprint::from_bytes([byte; 32])
}

fn facts(local: bool, epoch: u8) -> LinkFacts {
	LinkFacts {
		dialled_locally: local,
		epoch: [epoch; 16],
	}
}

#[test]
fn lower_fingerprint_dialler_wins_on_both_sides() {
	let (low, high) = (fp(1), fp(9));
	// The low peer dialled link X; the high peer dialled link Y.
	// From the low side: X is local-dialled, Y remote-dialled.
	let low_side = elect(&low, &high, facts(true, 1), facts(false, 2));
	// From the high side the same two links look mirrored.
	let high_side = elect(&high, &low, facts(false, 1), facts(true, 2));
	assert_eq!(low_side, Verdict::KeepFirst);
	assert_eq!(high_side, Verdict::KeepFirst);
	// Order of arrival does not matter.
	assert_eq!(
		elect(&low, &high, facts(false, 2), facts(true, 1)),
		Verdict::KeepSecond
	);
}

#[test]
fn same_dialler_prefers_higher_epoch_then_keeps_existing() {
	let (a, b) = (fp(1), fp(2));
	assert_eq!(
		elect(&a, &b, facts(true, 1), facts(true, 5)),
		Verdict::KeepSecond
	);
	assert_eq!(
		elect(&a, &b, facts(true, 5), facts(true, 1)),
		Verdict::KeepFirst
	);
	assert_eq!(
		elect(&a, &b, facts(true, 3), facts(true, 3)),
		Verdict::KeepFirst
	);
}

#[test]
fn backoff_grows_is_capped_and_jittered_within_bounds() {
	let cfg = BackoffConfig {
		base: Duration::from_millis(100),
		max: Duration::from_secs(2),
	};
	assert_eq!(cfg.delay(0, 1.0), Duration::from_millis(100));
	assert_eq!(cfg.delay(0, 0.0), Duration::from_millis(50));
	assert_eq!(cfg.delay(3, 1.0), Duration::from_millis(800));
	assert_eq!(cfg.delay(40, 1.0), Duration::from_secs(2));
	assert_eq!(cfg.delay(40, 0.0), Duration::from_secs(1));
	assert_eq!(cfg.delay(2, f64::NAN), cfg.delay(2, 1.0));
	let mut last = Duration::ZERO;
	for attempt in 0..8 {
		let d = cfg.delay(attempt, 1.0);
		assert!(d >= last);
		last = d;
	}
}

#[test]
fn outbound_invite_session_survives_the_round_trip() {
	let mut table = SessionTable::default();
	let (call, session) = (CallId::from_u128(11), SessionId::from_u128(22));
	let envelope = encode(
		&PeerMessage::Invite {
			call,
			session,
			allow_parallel_same_peer: true,
		},
		peer0(),
		&mut table,
	);
	// The remote answers with Accept for the same call.
	let accept = encode(
		&PeerMessage::Accept { call, session },
		peer0(),
		&mut SessionTable::default(),
	);
	let peer = relay_core::PeerId::from_bytes([3; 32]);
	let policy = AddressPolicy::lan_only();
	let decoded = decode(peer, accept.body.unwrap(), &mut table, &policy);
	assert_eq!(
		decoded,
		Inbound::Event(PeerEvent::Accepted { call, session })
	);
	// The remote sees our invite with a derived session and the flag intact.
	let mut remote = SessionTable::default();
	match decode(peer, envelope.body.unwrap(), &mut remote, &policy) {
		Inbound::Event(PeerEvent::Invite {
			call: c,
			session: s,
			allow_parallel_same_peer,
			..
		}) => {
			assert_eq!(
				(c, s, allow_parallel_same_peer),
				(call, derived_session(call), true)
			);
		}
		other => panic!("{other:?}"),
	}
}

#[test]
fn the_session_table_remembers_which_peer_a_call_belongs_to() {
	let mut table = SessionTable::default();
	let call = CallId::from_u128(31);
	table_remember(&mut table, call, SessionId::from_u128(32));
	assert_eq!(table.peer_of(call), Some(peer0()));
	assert_eq!(table.peer_of(CallId::from_u128(99)), None);
}

#[test]
fn unknown_calls_and_other_bodies_are_ignored() {
	let mut table = SessionTable::default();
	let policy = AddressPolicy::lan_only();
	let peer = relay_core::PeerId::from_bytes([3; 32]);
	let end = encode(
		&PeerMessage::End {
			call: CallId::from_u128(5),
			session: SessionId::from_u128(6),
		},
		peer0(),
		&mut SessionTable::default(),
	);
	assert_eq!(
		decode(peer, end.body.unwrap(), &mut table, &policy),
		Inbound::Ignored
	);
}

#[test]
fn decline_maps_busy_and_plain_to_declined_events() {
	let mut table = SessionTable::default();
	let (call, session) = (CallId::from_u128(1), SessionId::from_u128(2));
	table_remember(&mut table, call, session);
	let peer = relay_core::PeerId::from_bytes([3; 32]);
	for busy in [false, true] {
		let env = encode(
			&PeerMessage::Decline {
				call,
				session,
				busy,
			},
			peer0(),
			&mut SessionTable::default(),
		);
		assert_eq!(
			decode(
				peer,
				env.body.unwrap(),
				&mut table,
				&AddressPolicy::lan_only()
			),
			Inbound::Event(PeerEvent::Declined { call, session })
		);
	}
}

fn table_remember(table: &mut SessionTable, call: CallId, session: SessionId) {
	encode(
		&PeerMessage::Invite {
			call,
			session,
			allow_parallel_same_peer: false,
		},
		peer0(),
		table,
	);
}

fn peer0() -> relay_core::PeerId {
	relay_core::PeerId::from_bytes([3; 32])
}
