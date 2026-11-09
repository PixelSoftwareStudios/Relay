//! The push-to-talk worker against a scripted engine on a real socket.

#![cfg(unix)]

#[allow(dead_code)]
mod support;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_api::pb::{InputAction, InputControlMessage};
use relay_client::ptt::{PttState, PttTarget, PttWorker, Resolve};
use relay_client::wire::connect_registered;
use support::{Harness, InputReply};
use tokio::runtime::Handle;

const CALL: &str = "d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1";

struct Rig {
	harness: Harness,
	worker: PttWorker,
	target: Arc<Mutex<Option<PttTarget>>>,
	_client: relay_api::RelayClient,
}

async fn rig() -> Rig {
	let harness = Harness::start().await;
	let client = connect_registered(&harness.endpoint(), "test", "0", &["observe"])
		.await
		.unwrap();
	let target = Arc::new(Mutex::new(Some(PttTarget {
		call_id: vec![0xd1; 16],
		controller_generation: 5,
		transmit_route_generation: 7,
		client_id: client.context().unwrap().client_id,
	})));
	let slot = target.clone();
	let resolve: Resolve = Arc::new(move |call| {
		(call == CALL)
			.then(|| slot.lock().unwrap().clone())
			.flatten()
	});
	let worker = PttWorker::spawn(&Handle::current(), harness.endpoint(), resolve);
	Rig {
		harness,
		worker,
		target,
		_client: client,
	}
}

impl Rig {
	fn seen(&self) -> Vec<InputControlMessage> {
		self.harness.engine.inputs.lock().unwrap().clone()
	}

	fn actions(&self) -> Vec<InputAction> {
		self.seen()
			.iter()
			.map(|m| InputAction::try_from(m.action).unwrap())
			.collect()
	}

	async fn until(&self, what: &str, mut ready: impl FnMut(&Rig) -> bool) {
		for _ in 0..200 {
			if ready(self) {
				return;
			}
			tokio::time::sleep(Duration::from_millis(10)).await;
		}
		panic!("timed out waiting for {what}");
	}

	async fn state(&self, want: PttState) {
		let mut states = self.worker.states();
		for _ in 0..200 {
			if *states.borrow_and_update() == want {
				return;
			}
			tokio::time::sleep(Duration::from_millis(10)).await;
		}
		panic!("state {want:?}, was {:?}", *states.borrow());
	}
}

#[tokio::test]
async fn a_press_holds_a_lease_that_is_renewed_and_release_ends_it() {
	let rig = rig().await;
	rig.worker.press(CALL);
	rig.until("press", |r| {
		r.actions().first() == Some(&InputAction::Press)
	})
	.await;
	rig.state(PttState::Transmitting).await;
	let press = &rig.seen()[0];
	assert_eq!(press.call_id, vec![0xd1; 16]);
	assert_eq!(press.controller_generation, 5);
	assert_eq!(press.transmit_route_generation, 7);
	assert_eq!(press.requested_lease_ms, 400);
	// Longer than the lease: only renewals keep it alive.
	rig.until("two renewals", |r| {
		r.actions()
			.iter()
			.filter(|a| **a == InputAction::Renew)
			.count() >= 2
	})
	.await;
	rig.worker.release();
	rig.until("release", |r| {
		r.actions().last() == Some(&InputAction::Release)
	})
	.await;
	rig.state(PttState::Idle).await;
	let sequences: Vec<u64> = rig.seen().iter().map(|m| m.input_sequence).collect();
	assert!(
		sequences.windows(2).all(|w| w[1] == w[0] + 1),
		"{sequences:?}"
	);
	let before = rig.seen().len();
	tokio::time::sleep(Duration::from_millis(400)).await;
	assert_eq!(
		rig.seen().len(),
		before,
		"nothing is sent after the release"
	);
}

#[tokio::test]
async fn a_press_without_a_controlled_push_to_talk_call_sends_nothing() {
	let rig = rig().await;
	*rig.target.lock().unwrap() = None;
	rig.worker.press(CALL);
	rig.state(PttState::NoTarget).await;
	rig.worker.press("not-a-call");
	tokio::time::sleep(Duration::from_millis(200)).await;
	assert!(rig.seen().is_empty());
	assert_eq!(rig.harness.engine.streams_opened.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancel_closes_the_stream_and_the_next_press_opens_a_new_one() {
	let rig = rig().await;
	rig.worker.press(CALL);
	rig.state(PttState::Transmitting).await;
	rig.worker.cancel();
	rig.until("the stream to close", |r| {
		r.harness.engine.streams_closed.load(Ordering::SeqCst) == 1
	})
	.await;
	assert!(
		!rig.actions().contains(&InputAction::Release),
		"closing the stream is the release, not a message with a release delay"
	);
	rig.state(PttState::Idle).await;
	rig.worker.press(CALL);
	rig.until("a second stream", |r| {
		r.harness.engine.streams_opened.load(Ordering::SeqCst) == 2
	})
	.await;
}

#[tokio::test]
async fn dropping_the_worker_closes_the_stream() {
	let rig = rig().await;
	rig.worker.press(CALL);
	rig.state(PttState::Transmitting).await;
	let Rig {
		harness, worker, ..
	} = rig;
	drop(worker);
	for _ in 0..200 {
		if harness.engine.streams_closed.load(Ordering::SeqCst) == 1 {
			return;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!("the stream stayed open");
}

#[tokio::test]
async fn a_call_that_stops_being_a_push_to_talk_call_releases_at_once() {
	let rig = rig().await;
	rig.worker.press(CALL);
	rig.state(PttState::Transmitting).await;
	*rig.target.lock().unwrap() = None;
	rig.until("the stream to close", |r| {
		r.harness.engine.streams_closed.load(Ordering::SeqCst) == 1
	})
	.await;
	assert!(!rig.actions().contains(&InputAction::Release));
	rig.state(PttState::Idle).await;
}

#[tokio::test]
async fn a_changed_controller_generation_ends_the_press() {
	let rig = rig().await;
	rig.worker.press(CALL);
	rig.state(PttState::Transmitting).await;
	rig.target
		.lock()
		.unwrap()
		.as_mut()
		.unwrap()
		.controller_generation = 6;
	rig.until("the stream to close", |r| {
		r.harness.engine.streams_closed.load(Ordering::SeqCst) == 1
	})
	.await;
}

#[tokio::test]
async fn a_refusal_ends_the_press_and_is_reported() {
	let rig = rig().await;
	*rig.harness.engine.reply.lock().unwrap() = InputReply::Reject("not_controller");
	rig.worker.press(CALL);
	rig.state(PttState::Rejected {
		code: "not_controller".into(),
	})
	.await;
	let sent = rig.seen().len();
	tokio::time::sleep(Duration::from_millis(400)).await;
	assert_eq!(rig.seen().len(), sent, "no renewals after a refusal");
}

#[tokio::test]
async fn an_engine_that_is_not_transmitting_is_shown_as_gated() {
	let rig = rig().await;
	*rig.harness.engine.reply.lock().unwrap() = InputReply::Accept {
		transmitting: false,
	};
	rig.worker.press(CALL);
	rig.state(PttState::Gated).await;
}

#[tokio::test(start_paused = true)]
async fn a_stuck_key_is_released_at_once_after_a_minute() {
	let rig = rig().await;
	rig.worker.press(CALL);
	rig.until("press", |r| r.actions().contains(&InputAction::Press))
		.await;
	tokio::time::sleep(relay_client::ptt::MAX_HOLD + Duration::from_secs(1)).await;
	rig.until("the stream to close", |r| {
		r.harness.engine.streams_closed.load(Ordering::SeqCst) == 1
	})
	.await;
	rig.state(PttState::Idle).await;
	let sent = rig.seen().len();
	tokio::time::sleep(Duration::from_secs(1)).await;
	assert_eq!(rig.seen().len(), sent, "no more renewals once released");
}

#[tokio::test]
async fn release_interrupts_a_stalled_open_without_transmitting_a_queued_press() {
	let rig = rig().await;
	rig.harness
		.engine
		.input_delay_ms
		.store(500, Ordering::Relaxed);
	rig.worker.press(CALL);
	rig.until("pending stream open", |r| {
		r.harness.engine.streams_opened.load(Ordering::SeqCst) == 1
	})
	.await;
	for _ in 0..10_000 {
		rig.worker.press(CALL);
	}
	rig.worker.release();
	// The 500ms RPC is still blocked; releasing must not wait for it.
	tokio::time::timeout(Duration::from_millis(200), rig.state(PttState::Idle))
		.await
		.unwrap();
	tokio::time::sleep(Duration::from_millis(550)).await;
	assert!(
		rig.seen().is_empty(),
		"a released pending press must never be transmitted"
	);
}

#[tokio::test]
async fn cancel_followed_by_a_press_closes_the_old_source_even_when_coalesced() {
	let rig = rig().await;
	rig.worker.press(CALL);
	rig.state(PttState::Transmitting).await;
	rig.worker.cancel();
	rig.worker.press(CALL);
	rig.until("replacement source", |r| {
		r.harness.engine.streams_opened.load(Ordering::SeqCst) == 2
	})
	.await;
	rig.until("old source closed", |r| {
		r.harness.engine.streams_closed.load(Ordering::SeqCst) == 1
	})
	.await;
	rig.until("second press", |r| {
		r.seen()
			.iter()
			.filter(|m| m.action == InputAction::Press as i32)
			.count() == 2
	})
	.await;
	let presses: Vec<_> = rig
		.seen()
		.into_iter()
		.filter(|m| m.action == InputAction::Press as i32)
		.collect();
	assert_ne!(presses[0].source_id, presses[1].source_id);
}
