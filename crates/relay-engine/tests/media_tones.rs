//! RT-18: a tone plays through an otherwise idle render worker.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_audio::device::{AudioBackend, Direction, FakeBackend, FakeDeviceSpec, StreamRequest};
use relay_audio::tones::{Sound, TonePlayer};
use relay_engine::media::{
	Heartbeat, Inbox, LoadMeter, Meters, RenderCmd, RenderOutput, RenderSink, Supervision,
	WorkerId, monitor_pair, spawn_render_worker,
};

#[test]
fn a_ring_tone_is_rendered_with_no_call_and_wakes_an_idle_worker() {
	let mut backend = FakeBackend::new(vec![FakeDeviceSpec {
		is_default: true,
		..FakeDeviceSpec::new("spk", Direction::Output)
	}]);
	let handle = backend
		.enumerate()
		.unwrap()
		.of_direction(Direction::Output)
		.next()
		.unwrap()
		.handle;
	let stream = backend
		.open_playback(handle, &StreamRequest::default())
		.unwrap();
	stream.handle.play().unwrap();
	let mut driver = backend.take_playback_driver().unwrap();
	driver.enable_recording(48_000 * 4);

	let (inbox, commands) = Inbox::new(16);
	let stop = Arc::new(AtomicBool::new(false));
	let (_tx, monitor_rx) = monitor_pair();
	let thread = spawn_render_worker(
		commands,
		Arc::new(LoadMeter::default()),
		Arc::new(Meters::default()),
		monitor_rx,
		Supervision {
			id: WorkerId::Render,
			heartbeat: Arc::new(Heartbeat::default()),
			failed: Arc::new(AtomicBool::new(false)),
			stop: Arc::clone(&stop),
			report: Arc::new(|_| {}),
			promote: false,
		},
	)
	.unwrap();
	inbox.set_waker(thread.thread().clone());
	let sink = RenderSink::new(stream.writer, stream.config.sample_rate, 20).unwrap();
	assert!(
		inbox
			.push(RenderCmd::Attach(Box::new(RenderOutput {
				sink,
				health: stream.health,
				shared: stream.shared,
			})))
			.is_ok()
	);
	let (player, mut control) = TonePlayer::new();
	assert!(inbox.push(RenderCmd::SetTones(Box::new(player))).is_ok());
	// The worker is idle (no call, no monitor). Starting a tone must wake it.
	std::thread::sleep(Duration::from_millis(50));
	control.start(Sound::Ring, true, 0.5).unwrap();
	inbox.wake();
	let heard = Arc::new(Mutex::new(Vec::new()));
	for _ in 0..100 {
		std::thread::sleep(Duration::from_millis(10));
		driver.render(480);
	}
	heard.lock().unwrap().extend_from_slice(driver.recorded());
	let peak = heard
		.lock()
		.unwrap()
		.iter()
		.fold(0.0_f32, |m, s| m.max(s.abs()));
	assert!(
		peak > 0.01,
		"the ring tone reached the speaker, peak {peak}"
	);

	stop.store(true, std::sync::atomic::Ordering::Release);
	thread.thread().unpark();
	thread.join().unwrap();
}
