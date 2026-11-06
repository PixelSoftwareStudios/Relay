//! A microphone that could not be opened (no permission yet) is tried again
//! once the answer may have changed, without a restart.

mod common;

use common::Fixture;
use relay_core::*;

fn failed_open() -> Fixture {
	let mut f = Fixture::new();
	f.route(TransmitRoute::open_mic()).unwrap();
	let generation = f.node.device.opened[0];
	f.node.feed(f.node.device.failed(generation));
	assert_eq!(f.node.state.snapshot().global.capture, CaptureState::Lost);
	assert!(!f.gate_open());
	f
}

#[test]
fn a_grant_during_the_call_reopens_capture() {
	let mut f = failed_open();
	assert_eq!(f.node.device.opened.len(), 1, "no retry loop inside the call");
	f.node.feed(Input::System(SystemEvent::InputAccessChanged));
	assert_eq!(f.node.device.opened.len(), 2);
	f.node.complete_capture();
	assert!(f.gate_open());
}

#[test]
fn the_next_call_tries_again_after_a_failed_open() {
	let mut f = failed_open();
	f.route(TransmitRoute::RECEIVE_ONLY).unwrap();
	assert_eq!(f.node.state.snapshot().global.capture, CaptureState::Closed);
	f.route(TransmitRoute::open_mic()).unwrap();
	assert_eq!(f.node.device.opened.len(), 2);
	f.node.complete_capture();
	assert!(f.gate_open());
}

#[test]
fn a_lost_device_still_waits_for_an_explicit_resume() {
	let mut f = Fixture::new();
	f.route(TransmitRoute::open_mic()).unwrap();
	let generation = f.node.device.opened[0];
	f.node.complete_capture();
	f.node.feed(f.node.device.unplugged(generation));
	f.route(TransmitRoute::RECEIVE_ONLY).unwrap();
	f.route(TransmitRoute::open_mic()).unwrap();
	assert_eq!(f.node.state.snapshot().global.capture, CaptureState::Lost);
	assert_eq!(f.node.device.opened.len(), 1);
}

#[test]
fn an_access_change_with_nothing_lost_does_nothing() {
	let mut f = Fixture::new();
	let out = f.node.feed(Input::System(SystemEvent::InputAccessChanged));
	assert!(out.result.is_ok());
	assert_eq!(f.node.state.snapshot().global.capture, CaptureState::Closed);
}
