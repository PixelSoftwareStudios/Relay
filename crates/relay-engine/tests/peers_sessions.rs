//! F-05 and F-14 of the net/channels review: one peer cannot evict or hijack the
//! call-to-session state of others, and media candidates must sit on the link's network.

use relay_core::{CallId, PeerEvent, PeerId, PeerMessage, SessionId};
use relay_engine::peers::{
	Inbound, LinkNet, MediaSignal, SessionTable, Subnet, decode, derived_session, encode,
	encode_media,
};
use relay_net::AddressPolicy;
use relay_net::proto::envelope::Body;

fn peer(byte: u8) -> PeerId {
	PeerId::from_bytes([byte; 32])
}

fn net() -> LinkNet {
	LinkNet {
		remote_ip: [192, 168, 1, 9].into(),
		subnet: Some(Subnet::new([192, 168, 1, 20].into(), 24)),
	}
}

fn invite(call: u128) -> Body {
	let mut scratch = SessionTable::default();
	encode(
		&PeerMessage::Invite {
			call: CallId::from_u128(call),
			session: SessionId::from_u128(call),
			allow_parallel_same_peer: false,
		},
		peer(0),
		&mut scratch,
	)
	.body
	.unwrap()
}

fn end(call: u128) -> Body {
	encode(
		&PeerMessage::End {
			call: CallId::from_u128(call),
			session: SessionId::from_u128(call),
		},
		peer(0),
		&mut SessionTable::default(),
	)
	.body
	.unwrap()
}

fn ready(call: u128) -> Body {
	encode_media(&MediaSignal::Ready {
		call: CallId::from_u128(call),
		ssrc: 1,
		generation: 1,
		epoch: [7; 16],
	})
	.body
	.unwrap()
}

fn feed(table: &mut SessionTable, from: PeerId, body: Body) -> Inbound {
	decode(from, body, table, &AddressPolicy::lan_only(), &net())
}

#[test]
fn an_invite_flood_does_not_evict_the_calls_of_other_peers() {
	let mut table = SessionTable::default();
	let (victim, attacker) = (peer(1), peer(2));
	// A real call with the victim, then the attacker sends far more than 256 invites.
	assert!(matches!(
		feed(&mut table, victim, invite(1)),
		Inbound::Event(PeerEvent::Invite { .. })
	));
	for call in 1_000..1_400 {
		feed(&mut table, attacker, invite(call));
	}
	// The victim's hangup still reaches the domain.
	assert_eq!(
		feed(&mut table, victim, end(1)),
		Inbound::Event(PeerEvent::Ended {
			call: CallId::from_u128(1),
			session: derived_session(CallId::from_u128(1)),
		})
	);
}

#[test]
fn the_table_is_bounded_per_peer_even_without_the_rate_limit() {
	// Fill from many peers, each under its rate: the flooding peer only loses its own.
	let mut table = SessionTable::default();
	let (victim, attacker) = (peer(1), peer(2));
	feed(&mut table, victim, invite(1));
	for round in 0..30u8 {
		let from = peer(100 + round);
		for i in 0..9u128 {
			feed(
				&mut table,
				from,
				invite(10_000 + u128::from(round) * 100 + i),
			);
		}
	}
	for call in 0..40u128 {
		feed(&mut table, attacker, invite(50_000 + call));
	}
	assert_eq!(table.peer_of(CallId::from_u128(1)), Some(victim));
}

#[test]
fn another_peer_cannot_end_accept_or_reject_a_call_it_does_not_own() {
	let mut table = SessionTable::default();
	let (owner, other) = (peer(1), peer(2));
	feed(&mut table, owner, invite(5));
	assert_eq!(feed(&mut table, other, end(5)), Inbound::Ignored);
	assert_eq!(feed(&mut table, other, ready(5)), Inbound::Ignored);
	// A second peer replaying the call id cannot take the entry over either.
	assert_eq!(feed(&mut table, other, invite(5)), Inbound::Ignored);
	assert_eq!(table.peer_of(CallId::from_u128(5)), Some(owner));
	assert!(matches!(
		feed(&mut table, owner, end(5)),
		Inbound::Event(PeerEvent::Ended { .. })
	));
}

#[test]
fn media_ready_for_unknown_calls_never_reaches_the_media_early_table() {
	let mut table = SessionTable::default();
	for call in 0..64 {
		assert_eq!(feed(&mut table, peer(2), ready(call)), Inbound::Ignored);
	}
	feed(&mut table, peer(1), invite(77));
	assert!(matches!(
		feed(&mut table, peer(1), ready(77)),
		Inbound::Media(MediaSignal::Ready { .. })
	));
}

#[test]
fn an_invite_burst_beyond_the_rate_is_dropped_before_the_table() {
	let mut table = SessionTable::default();
	let delivered = (0..100)
		.filter(|call| {
			matches!(
				feed(&mut table, peer(4), invite(*call)),
				Inbound::Event(PeerEvent::Invite { .. })
			)
		})
		.count();
	assert!((1..=16).contains(&delivered), "{delivered} delivered");
}

#[test]
fn media_candidates_must_be_the_link_host_or_on_its_subnet() {
	use relay_net::proto::EndpointOffer;
	let candidates = [
		[192, 168, 1, 9],     // the link's own host
		[192, 168, 1, 77],    // same /24
		[192, 168, 2, 1],     // another LAN host
		[10, 0, 0, 1],        // elsewhere
		[169, 254, 169, 254], // cloud metadata
	];
	let offer = Body::EndpointOffer(EndpointOffer {
		link_epoch: vec![7; 16],
		candidates: candidates
			.iter()
			.map(|ip| relay_net::endpoint_to_proto((*ip, 4000).into()))
			.collect(),
	});
	let mut table = SessionTable::default();
	let Inbound::Media(MediaSignal::Offer { candidates, .. }) = feed(&mut table, peer(1), offer)
	else {
		panic!("offer expected");
	};
	let ips: Vec<_> = candidates.iter().map(|c| c.ip().to_string()).collect();
	assert_eq!(ips, ["192.168.1.9", "192.168.1.77"]);
}
