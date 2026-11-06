//! A microphone that cannot be opened because the permission is missing opens
//! after the permission arrives, on the same running device thread, and the
//! engine says so (`CaptureOpened` withdraws the earlier warning).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use relay_audio::device::{
	AudioBackend, CaptureStream, DeviceHandle, Direction, Enumeration, FakeBackend, FakeDeviceSpec,
	PlaybackStream, StreamRequest,
};
use relay_core::{AudioGeneration, SystemEvent};
use relay_engine::media::{
	DeviceCmd, DeviceSetup, DeviceThread, EventSink, Inbox, MediaEvent, Selection,
};

/// A backend that refuses to record until `granted` is set, like a phone
/// whose microphone permission was just switched on.
struct Permissioned {
	inner: FakeBackend,
	granted: Arc<AtomicBool>,
}

impl AudioBackend for Permissioned {
	fn enumerate(&mut self) -> relay_audio::Result<Enumeration> {
		self.inner.enumerate()
	}

	fn open_capture(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> relay_audio::Result<CaptureStream> {
		if !self.granted.load(Ordering::SeqCst) {
			return Err(relay_audio::AudioError::Backend(
				"permission denied".to_owned(),
			));
		}
		self.inner.open_capture(device, request)
	}

	fn open_playback(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> relay_audio::Result<PlaybackStream> {
		self.inner.open_playback(device, request)
	}
}

fn wait(condition: impl Fn() -> bool) {
	let end = Instant::now() + Duration::from_secs(6);
	while !condition() {
		assert!(Instant::now() < end, "condition not reached in time");
		std::thread::sleep(Duration::from_millis(10));
	}
}

#[test]
fn capture_opens_once_the_permission_is_granted_without_a_restart() {
	let granted = Arc::new(AtomicBool::new(false));
	let events = Arc::new(Mutex::new(Vec::<MediaEvent>::new()));
	let sink: EventSink = {
		let events = Arc::clone(&events);
		Arc::new(move |event| events.lock().unwrap().push(event))
	};
	let (capture, _capture_rx) = Inbox::new(16);
	let (render, _render_rx) = Inbox::new(16);
	let backend_granted = Arc::clone(&granted);
	let thread = DeviceThread::spawn(
		Box::new(move || {
			Box::new(Permissioned {
				inner: FakeBackend::new(vec![FakeDeviceSpec {
					is_default: true,
					..FakeDeviceSpec::new("Mic", Direction::Input)
				}]),
				granted: backend_granted,
			})
		}),
		DeviceSetup {
			request: StreamRequest::default(),
			selection: Selection::default(),
			playout_target_ms: 20,
			buffers: Default::default(),
		},
		capture,
		render,
		sink,
	)
	.unwrap();
	let open = |generation: u64| {
		thread
			.commands
			.send(DeviceCmd::OpenCapture(AudioGeneration::new(generation)))
			.unwrap();
	};
	let has = |wanted: fn(&MediaEvent) -> bool| events.lock().unwrap().iter().any(wanted);

	open(1);
	wait(|| has(|e| matches!(e, MediaEvent::System(SystemEvent::CaptureFailed { .. }))));
	assert!(has(|e| matches!(e, MediaEvent::CaptureUnavailable(_))));
	assert!(!has(|e| matches!(e, MediaEvent::CaptureOpened)));

	granted.store(true, Ordering::SeqCst);
	open(2);
	wait(|| has(|e| matches!(e, MediaEvent::CaptureOpened)));
	assert!(has(|e| matches!(
		e,
		MediaEvent::System(SystemEvent::CaptureReady { .. })
	)));
	std::mem::forget((_capture_rx, _render_rx));
}
