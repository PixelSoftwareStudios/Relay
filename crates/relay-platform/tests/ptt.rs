//! PTT driver behaviour against a fake lease sink and the fake hotkey backend.

use std::time::Duration;

use relay_api::pb::{ClientContext, InputAction};
use relay_platform::ptt::BlockReason;
use relay_platform::session::SessionEvent;
use relay_platform::testing::{FakeBackend, FakeKeyboard, FakeLeaseSink, Reply};
use relay_platform::{
	BackendStatus, HotkeyBackend, HotkeyBinding, HotkeyEvent, PttConfig, PttDriver, PttError,
	PttInput, PttState, PttTarget,
};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::sleep;

struct Rig {
	keys: FakeKeyboard,
	inputs: mpsc::UnboundedSender<PttInput>,
	state: watch::Receiver<PttState>,
	sink: FakeLeaseSink,
	task: JoinHandle<Result<(), PttError>>,
	_backend: FakeBackend,
}

fn target(generation: u64) -> PttTarget {
	PttTarget {
		call_id: vec![7; 16],
		controller_generation: generation,
		transmit_route_generation: 1,
	}
}

async fn rig(sink: FakeLeaseSink) -> Rig {
	let (mut backend, keys) = FakeBackend::new(BackendStatus::Available);
	let stream = backend
		.start(&HotkeyBinding::push_to_talk("Ctrl+Space"))
		.unwrap();
	let (inputs, rx) = mpsc::unbounded_channel();
	let client = ClientContext {
		client_id: vec![1; 16],
		engine_incarnation: vec![2; 16],
	};
	let (driver, state) = PttDriver::new(sink.clone(), client, PttConfig::default());
	let task = tokio::spawn(driver.run(stream, rx));
	inputs.send(PttInput::Target(Some(target(1)))).unwrap();
	settle().await; // the driver must see the target before any key edge
	Rig {
		keys,
		inputs,
		state,
		sink,
		task,
		_backend: backend,
	}
}

async fn settle() {
	sleep(Duration::from_millis(1)).await;
}

#[tokio::test(start_paused = true)]
async fn press_and_release() {
	let r = rig(FakeLeaseSink::accepting()).await;
	r.keys.send(HotkeyEvent::Pressed);
	settle().await;
	assert_eq!(*r.state.borrow(), PttState::Transmitting);
	r.keys.send(HotkeyEvent::Released);
	settle().await;
	assert_eq!(*r.state.borrow(), PttState::Idle);
	assert_eq!(r.sink.actions(), [InputAction::Press, InputAction::Release]);
	let seen = r.sink.seen();
	assert_eq!(seen[0].call_id, vec![7; 16]);
	assert_eq!(seen[0].controller_generation, 1);
}

#[tokio::test(start_paused = true)]
async fn lease_renews_with_increasing_sequence() {
	let r = rig(FakeLeaseSink::accepting()).await;
	r.keys.send(HotkeyEvent::Pressed);
	sleep(Duration::from_millis(1000)).await;
	let seen = r.sink.seen();
	assert!(
		seen.len() >= 6,
		"expected several renewals, got {}",
		seen.len()
	);
	assert_eq!(seen[0].action, InputAction::Press as i32);
	assert!(
		seen[1..]
			.iter()
			.all(|m| m.action == InputAction::Renew as i32)
	);
	let seqs: Vec<u64> = seen.iter().map(|m| m.input_sequence).collect();
	assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1));
	assert_eq!(seqs[0], 1);
	assert!(seen.iter().all(|m| m.source_id == seen[0].source_id));
	assert_eq!(*r.state.borrow(), PttState::Transmitting);
}

#[tokio::test(start_paused = true)]
async fn stalled_engine_expires_and_fails_closed() {
	let sink = FakeLeaseSink::accepting();
	let r = rig(sink.clone()).await;
	r.keys.send(HotkeyEvent::Pressed);
	settle().await;
	sink.set_default(Reply::Stall);
	sleep(Duration::from_millis(500)).await;
	assert_eq!(*r.state.borrow(), PttState::Expired);
	// A release-all was still attempted, and the held key does not retransmit.
	assert_eq!(sink.actions().last(), Some(&InputAction::ReleaseAll));
	let before = sink.seen().len();
	r.keys.send(HotkeyEvent::Pressed);
	sleep(Duration::from_millis(500)).await;
	assert_eq!(sink.seen().len(), before);
}

#[tokio::test(start_paused = true)]
async fn rejection_is_reported_and_not_retried() {
	let sink = FakeLeaseSink::default();
	sink.push(Reply::Reject("NOT_CONTROLLER"));
	let r = rig(sink.clone()).await;
	r.keys.send(HotkeyEvent::Pressed);
	sleep(Duration::from_millis(600)).await;
	assert_eq!(
		*r.state.borrow(),
		PttState::Rejected {
			code: "NOT_CONTROLLER".into()
		}
	);
	assert_eq!(sink.actions(), [InputAction::Press]);
	r.keys.send(HotkeyEvent::Released);
	settle().await;
	assert_eq!(*r.state.borrow(), PttState::Idle);
}

#[tokio::test(start_paused = true)]
async fn gated_when_engine_not_transmitting() {
	let sink = FakeLeaseSink::default();
	sink.set_default(Reply::Accept {
		lease_ms: 300,
		transmitting: false,
	});
	let r = rig(sink).await;
	r.keys.send(HotkeyEvent::Pressed);
	settle().await;
	assert_eq!(*r.state.borrow(), PttState::Gated);
}

#[tokio::test(start_paused = true)]
async fn press_without_target_is_blocked() {
	let r = rig(FakeLeaseSink::accepting()).await;
	r.inputs.send(PttInput::Target(None)).unwrap();
	settle().await;
	r.keys.send(HotkeyEvent::Pressed);
	settle().await;
	assert_eq!(*r.state.borrow(), PttState::Blocked(BlockReason::NoTarget));
	assert!(r.sink.seen().is_empty());
}

#[tokio::test(start_paused = true)]
async fn lock_releases_and_requires_repress() {
	let r = rig(FakeLeaseSink::accepting()).await;
	r.keys.send(HotkeyEvent::Pressed);
	settle().await;
	r.inputs
		.send(PttInput::Session(SessionEvent::Locked))
		.unwrap();
	settle().await;
	assert_eq!(r.sink.actions().last(), Some(&InputAction::ReleaseAll));
	assert_eq!(
		*r.state.borrow(),
		PttState::Blocked(BlockReason::SessionLocked)
	);
	r.inputs
		.send(PttInput::Session(SessionEvent::Unlocked))
		.unwrap();
	sleep(Duration::from_millis(500)).await;
	// Key still held across the lock: no new lease until released and pressed again.
	assert_eq!(*r.state.borrow(), PttState::Idle);
	let count = r.sink.seen().len();
	r.keys.send(HotkeyEvent::Released);
	r.keys.send(HotkeyEvent::Pressed);
	settle().await;
	assert_eq!(r.sink.seen().len(), count + 1);
	assert_eq!(*r.state.borrow(), PttState::Transmitting);
}

#[tokio::test(start_paused = true)]
async fn focus_loss_and_target_change_release() {
	let r = rig(FakeLeaseSink::accepting()).await;
	r.keys.send(HotkeyEvent::Pressed);
	settle().await;
	r.inputs
		.send(PttInput::Session(SessionEvent::FocusLost))
		.unwrap();
	settle().await;
	assert_eq!(r.sink.actions().last(), Some(&InputAction::ReleaseAll));
	r.keys.send(HotkeyEvent::Released);
	r.keys.send(HotkeyEvent::Pressed);
	settle().await;
	r.inputs.send(PttInput::Target(Some(target(2)))).unwrap();
	settle().await;
	assert_eq!(r.sink.actions().last(), Some(&InputAction::ReleaseAll));
	assert_eq!(*r.state.borrow(), PttState::Idle);
}

#[tokio::test(start_paused = true)]
async fn disconnect_releases_and_ends() {
	let r = rig(FakeLeaseSink::accepting()).await;
	r.keys.send(HotkeyEvent::Pressed);
	settle().await;
	r.inputs
		.send(PttInput::Session(SessionEvent::Disconnected))
		.unwrap();
	let result = r.task.await.unwrap();
	assert!(matches!(result, Err(PttError::Disconnected)));
	assert_eq!(r.sink.actions().last(), Some(&InputAction::ReleaseAll));
	assert!(matches!(*r.state.borrow(), PttState::Failed(_)));
}

#[tokio::test(start_paused = true)]
async fn sink_error_fails_closed() {
	let sink = FakeLeaseSink::accepting();
	let r = rig(sink.clone()).await;
	r.keys.send(HotkeyEvent::Pressed);
	settle().await;
	sink.set_default(Reply::Error);
	let result = r.task.await.unwrap();
	assert!(matches!(result, Err(PttError::Sink(_))));
	assert!(matches!(*r.state.borrow(), PttState::Failed(_)));
}

#[tokio::test(start_paused = true)]
async fn backend_stop_releases() {
	let mut r = rig(FakeLeaseSink::accepting()).await;
	r.keys.send(HotkeyEvent::Pressed);
	settle().await;
	r._backend.stop();
	let result = r.task.await.unwrap();
	assert!(matches!(result, Err(PttError::BackendClosed)));
	assert_eq!(r.sink.actions().last(), Some(&InputAction::ReleaseAll));
}
