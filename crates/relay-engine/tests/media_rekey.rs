//! F-15: a call that spans a link re-establishment keeps its audio. Two real
//! engines; the link is replaced mid-call by a fresh handshake.

mod support_e2e;
mod support_media;

use std::time::Duration;

use relay_core::*;
use support_e2e::{Node, Pair, active_call, run_for, until};
use support_media::{power_at, rms};

fn tail(node: &Node, ms: usize) -> Vec<f32> {
	let heard = node.sim.heard();
	heard[heard.len().saturating_sub(48 * ms)..].to_vec()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn audio_continues_after_the_link_session_is_replaced() {
	let Pair {
		mut a,
		mut b,
		call,
		lease_a,
		..
	} = active_call().await;
	a.command(Command::SetTransmitRoute {
		call,
		lease: lease_a,
		route: TransmitRoute::open_mic(),
	})
	.unwrap();
	until(&mut a, &mut b, "A transmitting", |a, _| {
		a.gate_open(call) && a.media.report(call).is_some_and(|r| r.sent > 20)
	})
	.await;
	let before = a.media.media_epoch(b.id).expect("a media socket exists");

	// The link dialled by the lower fingerprint survives an election, so that
	// side replaces the session.
	let a_is_lower = a.identity.fingerprint().as_bytes() < b.identity.fingerprint().as_bytes();
	if a_is_lower {
		a.peers.dial(b.peers.local_addr(), Some(b.id)).await.unwrap();
	} else {
		b.peers.dial(a.peers.local_addr(), Some(a.id)).await.unwrap();
	}
	until(&mut a, &mut b, "both media sockets follow the new session", |a, b| {
		a.media.media_epoch(b.id).is_some_and(|e| e != before)
			&& b.media.media_epoch(a.id).is_some_and(|e| e != before)
	})
	.await;

	let sent = a.media.report(call).unwrap().sent;
	run_for(&mut a, &mut b, Duration::from_millis(1_500)).await;
	assert!(
		a.media.report(call).unwrap().sent > sent + 50,
		"A keeps sending on the new epoch"
	);
	let heard = tail(&b, 500);
	assert!(rms(&heard) > 0.05, "B still hears A");
	assert!(power_at(&heard, 440.0) > 20.0 * power_at(&heard, 1_700.0));
}
