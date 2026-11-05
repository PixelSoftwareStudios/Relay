//! F-05(b): early `Ready` messages age out and are capped per peer.

use std::time::{Duration, Instant};

use relay_core::{CallId, PeerId};
use relay_engine::media::{EARLY_PER_PEER, EARLY_TOTAL, EARLY_TTL, EarlyReady, RemoteReady};

const READY: RemoteReady = RemoteReady {
	ssrc: 1,
	generation: 1,
};

fn peer(n: u128) -> PeerId {
	let mut bytes = [0u8; 32];
	bytes[..16].copy_from_slice(&n.to_be_bytes());
	PeerId::from_bytes(bytes)
}

#[test]
fn entries_expire_and_are_not_returned_late() {
	let t0 = Instant::now();
	let mut early = EarlyReady::default();
	assert!(early.insert(CallId::from_u128(1), peer(1), [7; 16], READY, t0));
	assert!(
		early
			.take(&CallId::from_u128(1), t0 + EARLY_TTL / 2)
			.is_some()
	);
	assert!(early.insert(CallId::from_u128(2), peer(1), [7; 16], READY, t0));
	assert!(
		early
			.take(
				&CallId::from_u128(2),
				t0 + EARLY_TTL + Duration::from_millis(1)
			)
			.is_none()
	);
	assert!(early.is_empty());
}

#[test]
fn one_peer_cannot_fill_the_table() {
	let t0 = Instant::now();
	let mut early = EarlyReady::default();
	for n in 0..EARLY_PER_PEER as u128 {
		assert!(early.insert(CallId::from_u128(n), peer(1), [0; 16], READY, t0));
	}
	assert!(!early.insert(CallId::from_u128(99), peer(1), [0; 16], READY, t0));
	// Another peer still gets room, up to the overall cap.
	let mut n = 100;
	while early.len() < EARLY_TOTAL {
		assert!(early.insert(CallId::from_u128(n), peer(n), [0; 16], READY, t0));
		n += 1;
	}
	assert!(!early.insert(CallId::from_u128(n), peer(n), [0; 16], READY, t0));
	// Time frees the space again.
	assert!(early.insert(
		CallId::from_u128(n),
		peer(n),
		[0; 16],
		READY,
		t0 + EARLY_TTL * 2
	));
}
