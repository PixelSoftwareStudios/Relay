#![allow(dead_code)]

//! A real capture worker fed by a fake microphone, with a recording wire.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use relay_audio::capture::CaptureChain;
use relay_audio::device::{
	AudioBackend, Direction, FakeBackend, FakeCaptureDriver, FakeDeviceSpec, StreamRequest,
};
use relay_audio::dsp::NoiseSuppressor;
use relay_core::AudioGeneration;
use relay_engine::media::{
	CallSender, CaptureCmd, CaptureInput, EventSink, Heartbeat, Inbox, LoadMeter, MediaEvent,
	Meters, Supervision, WorkerId, spawn_capture_worker,
};
use relay_protocol::rtp::RtpProfile;

use crate::support_media::Wire;

pub const GENERATION: AudioGeneration = AudioGeneration::new(1);
pub const TIMESTAMP_BASE: u32 = 1000;

pub struct Rig {
	pub inbox: Inbox<CaptureCmd>,
	pub events: Arc<Mutex<Vec<MediaEvent>>>,
	pub meters: Arc<Meters>,
	pub heartbeat: Arc<Heartbeat>,
	pub failed: Arc<AtomicBool>,
	pub wire: Wire,
	pub driver: FakeCaptureDriver,
	backend: FakeBackend,
	stop: Arc<AtomicBool>,
	thread: Option<JoinHandle<()>>,
}

impl Rig {
	pub fn start(suppressor: Box<dyn NoiseSuppressor>) -> Self {
		let mut backend = FakeBackend::new(vec![FakeDeviceSpec {
			is_default: true,
			max_callback_frames: 4096,
			..FakeDeviceSpec::new("mic", Direction::Input)
		}]);
		let events = Arc::new(Mutex::new(Vec::new()));
		let sink: EventSink = {
			let events = Arc::clone(&events);
			Arc::new(move |event| events.lock().unwrap().push(event))
		};
		let (inbox, commands) = Inbox::new(64);
		let meters = Arc::new(Meters::default());
		let (monitor_tx, _monitor_rx) = relay_engine::media::monitor_pair();
		let stop = Arc::new(AtomicBool::new(false));
		let heartbeat = Arc::new(Heartbeat::default());
		let failed = Arc::new(AtomicBool::new(false));
		let report: Arc<dyn Fn(relay_engine::media::WorkerFailure) + Send + Sync> = {
			let sink = Arc::clone(&sink);
			Arc::new(move |f| sink(MediaEvent::WorkerFailed(f)))
		};
		let thread = spawn_capture_worker(
			commands,
			suppressor,
			sink,
			Arc::new(LoadMeter::default()),
			Arc::clone(&meters),
			monitor_tx,
			Supervision {
				id: WorkerId::Capture,
				heartbeat: Arc::clone(&heartbeat),
				failed: Arc::clone(&failed),
				stop: Arc::clone(&stop),
				report,
				promote: false,
			},
		)
		.unwrap();
		inbox.set_waker(thread.thread().clone());
		let enumeration = backend.enumerate().unwrap();
		let handle = enumeration
			.of_direction(Direction::Input)
			.next()
			.unwrap()
			.handle;
		let stream = backend
			.open_capture(handle, &StreamRequest::default())
			.unwrap();
		let driver = backend.take_capture_driver().unwrap();
		let chain = CaptureChain::new(stream.reader, stream.config.sample_rate).unwrap();
		let input = CaptureInput {
			chain,
			generation: GENERATION,
			health: stream.health,
			shared: stream.shared,
		};
		let _keep = stream.handle;
		std::mem::forget(_keep);
		let mut rig = Self {
			inbox,
			events,
			meters,
			heartbeat,
			failed,
			wire: Wire::default(),
			driver,
			backend,
			stop,
			thread: Some(thread),
		};
		rig.push(CaptureCmd::Attach(Box::new(input)));
		rig
	}

	/// Opens a fresh microphone stream and attaches it under `generation`.
	pub fn reattach(&mut self, generation: AudioGeneration) {
		let enumeration = self.backend.enumerate().unwrap();
		let handle = enumeration
			.of_direction(Direction::Input)
			.next()
			.unwrap()
			.handle;
		let stream = self
			.backend
			.open_capture(handle, &StreamRequest::default())
			.unwrap();
		self.driver = self.backend.take_capture_driver().unwrap();
		let chain = CaptureChain::new(stream.reader, stream.config.sample_rate).unwrap();
		std::mem::forget(stream.handle);
		self.push(CaptureCmd::Attach(Box::new(CaptureInput {
			chain,
			generation,
			health: stream.health,
			shared: stream.shared,
		})));
	}

	pub fn push(&mut self, command: CaptureCmd) {
		assert!(self.inbox.push(command).is_ok(), "command queue full");
	}

	pub fn add_sender(&mut self, sender: CallSender) {
		self.push(CaptureCmd::AddSender(Box::new(sender)));
	}

	pub fn is_alive(&self) -> bool {
		self.thread.as_ref().is_some_and(|t| !t.is_finished())
	}

	/// Sample indices (device timeline) of every frame sent so far.
	pub fn sent_first_samples(&self) -> Vec<u64> {
		self.wire
			.snapshot()
			.iter()
			.map(|p| {
				let header = RtpProfile::V1.parse(p.rtp()).unwrap().header;
				u64::from(header.timestamp.wrapping_sub(TIMESTAMP_BASE))
			})
			.collect()
	}

	pub fn events(&self) -> Vec<MediaEvent> {
		self.events.lock().unwrap().clone()
	}

	/// Waits until `condition` holds or panics after `limit`.
	pub fn wait_for(&self, what: &str, limit: Duration, condition: impl Fn(&Self) -> bool) {
		let start = Instant::now();
		while !condition(self) {
			assert!(start.elapsed() < limit, "timed out waiting for {what}");
			std::thread::sleep(Duration::from_millis(2));
		}
	}
}

impl Drop for Rig {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::Release);
		if let Some(thread) = self.thread.take() {
			thread.thread().unpark();
			let _ = thread.join();
		}
		let _ = &self.backend;
	}
}
