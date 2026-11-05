//! RT-02: audio captured before the gate opened is never transmitted, however
//! the worker is scheduled. Uses the real capture worker thread, a fake
//! microphone and a recording wire.

mod support_capture;
mod support_media;

use std::time::Duration;

use relay_audio::device::Signal;
use relay_audio::dsp::{
	Discontinuity, DspDescriptor, DspError, DspLimits, NoiseSuppressor, OffSuppressor,
	ProcessProgress,
};
use relay_audio::format::BlockTiming;
use relay_core::{AudioGeneration, CallId};
use relay_engine::media::Emit;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use support_capture::Rig;
use support_media::{Wire, closed_gate, open_gate, revision, sender, tone};

const CALL: CallId = CallId::from_u128(1);
const MIC: Signal = Signal::Sine {
	freq_hz: 440.0,
	amplitude: 0.3,
};

/// Wraps the real pass-through but can be told to block inside `process_into`,
/// which stalls the capture worker mid-poll exactly like a slow suppressor.
struct Stallable {
	inner: OffSuppressor,
	stall: Arc<AtomicBool>,
	entered: Arc<AtomicBool>,
}

impl NoiseSuppressor for Stallable {
	fn descriptor(&self) -> &DspDescriptor {
		self.inner.descriptor()
	}

	fn prepare(&mut self, limits: &DspLimits) -> Result<(), DspError> {
		self.inner.prepare(limits)
	}

	fn process_into(
		&mut self,
		input: &[f32],
		output: &mut [f32],
		timing: BlockTiming,
	) -> Result<ProcessProgress, DspError> {
		if self.stall.load(Ordering::Acquire) {
			self.entered.store(true, Ordering::Release);
			while self.stall.load(Ordering::Acquire) {
				std::thread::sleep(Duration::from_millis(1));
			}
		}
		self.inner.process_into(input, output, timing)
	}

	fn reset(&mut self, reason: Discontinuity) {
		self.inner.reset(reason);
	}
}

#[test]
fn a_frame_stamped_in_an_unclean_poll_is_refused_even_though_the_gate_is_now_open() {
	// The review's `gate` scratch program: publish an open snapshot with identical
	// generations and emit a frame captured before it. It used to return `Sent`.
	let wire = Wire::default();
	let (mut sender, gate) = sender(CALL, 7, &wire);
	gate.publish(closed_gate(CALL, 1, 1), revision(1));
	let before_press = sender.stamp(AudioGeneration::new(1), false);
	gate.publish(open_gate(CALL, 1, 1), revision(2));
	assert_eq!(sender.emit(&tone(440.0, 0), before_press, 0), Emit::Unarmed);
	assert_eq!(wire.len(), 0);
}

#[test]
fn a_poll_is_armed_only_after_the_gate_stayed_open_for_a_whole_poll_interval() {
	let wire = Wire::default();
	let (mut sender, gate) = sender(CALL, 7, &wire);
	let capture = AudioGeneration::new(1);
	gate.publish(open_gate(CALL, 1, 1), revision(1));
	let first = sender.stamp(capture, false);
	assert!(
		!first.clean,
		"the first poll that sees the gate open is not trusted"
	);
	let second = sender.stamp(capture, false);
	assert!(second.clean);
	assert_eq!(sender.emit(&tone(440.0, 0), second, 0), Emit::Sent);
	// A stall also withdraws trust: the backlog's age is unknown.
	assert!(!sender.stamp(capture, true).clean);
}

#[test]
fn audio_still_in_the_pipeline_when_a_dirty_poll_ends_is_refused_later() {
	let wire = Wire::default();
	let (mut sender, gate) = sender(CALL, 7, &wire);
	let capture = AudioGeneration::new(1);
	gate.publish(open_gate(CALL, 1, 1), revision(1));
	let dirty = sender.stamp(capture, false);
	// The dirty poll drained samples up to 1000 (plus 100 of group delay).
	sender.end_poll(dirty, 1_000, 100);
	let clean = sender.stamp(capture, false);
	assert!(clean.clean);
	assert_eq!(sender.emit(&tone(440.0, 0), clean, 960), Emit::Unarmed);
	assert_eq!(sender.emit(&tone(440.0, 1), clean, 1_440), Emit::Sent);
}

#[test]
fn nothing_captured_before_the_press_is_sent_when_the_worker_keeps_up() {
	let mut rig = Rig::start(Box::new(OffSuppressor::default()));
	rig.driver.set_signal(MIC);
	let (sender, gate) = sender(CALL, 7, &rig.wire);
	// A push-to-talk press changes `open` only: route and capture generations
	// stay equal, which is what let pre-press frames through.
	gate.publish(closed_gate(CALL, 1, 1), revision(1));
	gate.publish(closed_gate(CALL, 1, 1), revision(1));
	rig.add_sender(sender);
	for _ in 0..30 {
		rig.driver.run(300);
		std::thread::sleep(Duration::from_millis(3));
	}
	assert_eq!(rig.wire.len(), 0, "closed gate sends nothing");
	// Press: the microphone has been running the whole time.
	gate.publish(open_gate(CALL, 1, 1), revision(2));
	let press = rig.driver.next_index();
	for _ in 0..60 {
		rig.driver.run(300);
		std::thread::sleep(Duration::from_millis(3));
	}
	rig.wait_for("audio after the press", Duration::from_secs(2), |r| {
		r.wire.len() > 10
	});
	let firsts = rig.sent_first_samples();
	assert!(
		firsts.iter().all(|first| *first >= press),
		"a frame starting before the press ({press}) was sent: {firsts:?}"
	);
}

#[test]
fn a_press_during_a_worker_stall_does_not_release_the_backlog() {
	let stall = Arc::new(AtomicBool::new(false));
	let entered = Arc::new(AtomicBool::new(false));
	let mut rig = Rig::start(Box::new(Stallable {
		inner: OffSuppressor::default(),
		stall: Arc::clone(&stall),
		entered: Arc::clone(&entered),
	}));
	rig.driver.set_signal(MIC);
	let (sender, gate) = sender(CALL, 7, &rig.wire);
	// A push-to-talk press changes `open` only: route and capture generations
	// stay equal, which is what let pre-press frames through.
	gate.publish(closed_gate(CALL, 1, 1), revision(1));
	rig.add_sender(sender);
	// Establish steady state with the gate closed and the mic running.
	for _ in 0..20 {
		rig.driver.run(480);
		std::thread::sleep(Duration::from_millis(3));
	}
	// Stall the worker inside a poll, keep the microphone running, press.
	stall.store(true, Ordering::Release);
	rig.driver.run(480);
	rig.wait_for("worker to stall", Duration::from_secs(2), |_| {
		entered.load(Ordering::Acquire)
	});
	for _ in 0..12 {
		rig.driver.run(480); // 120 ms of audio queues up behind the stall
	}
	gate.publish(open_gate(CALL, 1, 1), revision(2));
	let press = rig.driver.next_index();
	stall.store(false, Ordering::Release);
	for _ in 0..60 {
		rig.driver.run(480);
		std::thread::sleep(Duration::from_millis(3));
	}
	rig.wait_for("audio after the stall", Duration::from_secs(2), |r| {
		r.wire.len() > 10
	});
	let firsts = rig.sent_first_samples();
	assert!(
		firsts.iter().all(|first| *first >= press),
		"backlog captured before the press ({press}) was sent: {firsts:?}"
	);
}
