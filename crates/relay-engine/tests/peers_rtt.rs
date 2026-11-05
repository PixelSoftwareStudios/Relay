//! Round-trip timing from authenticated keepalives.

mod support_peers;

use std::time::Duration;

use relay_engine::peers::RttCell;
use support_peers::linked_pair;

#[test]
fn only_the_latest_probe_is_timed_and_each_echo_counts_once() {
	let cell = RttCell::default();
	assert_eq!(cell.get(), None);
	cell.probe_echoed(1);
	assert_eq!(cell.get(), None, "an echo with no probe out is ignored");
	cell.probe_sent(5);
	cell.probe_echoed(4);
	assert_eq!(cell.get(), None, "a stale or forged nonce is ignored");
	std::thread::sleep(Duration::from_millis(5));
	cell.probe_echoed(5);
	let first = cell.get().expect("timed");
	assert!(first >= Duration::from_millis(5), "{first:?}");
	cell.probe_echoed(5);
	assert_eq!(cell.get(), Some(first), "a replayed echo changes nothing");
	cell.probe_sent(6);
	cell.probe_echoed(6);
	assert!(
		cell.get().unwrap() < first,
		"the smoothed value follows new samples"
	);
	cell.probe_echoed(0);
}

#[tokio::test]
async fn a_linked_pair_measures_a_small_round_trip_on_both_sides() {
	let (a, b) = linked_pair().await;
	for _ in 0..100 {
		if a.handle.rtt(b.id()).is_some() && b.handle.rtt(a.id()).is_some() {
			break;
		}
		tokio::time::sleep(Duration::from_millis(50)).await;
	}
	for (here, there) in [(&a, &b), (&b, &a)] {
		let rtt = here.handle.rtt(there.id()).expect("a keepalive was echoed");
		assert!(rtt < Duration::from_millis(250), "loopback, got {rtt:?}");
	}
}
