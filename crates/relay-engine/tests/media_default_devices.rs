//! Nothing has to be chosen: the device thread starts on the system default,
//! a pinned device wins, a vanished pinned device falls back to the default
//! (and the state says so), and a changed default is picked up when the next
//! stream opens.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use relay_audio::device::{
	AudioBackend, CaptureStream, DeviceHandle, DeviceSelection, Direction, Enumeration,
	FakeBackend, FakeDeviceSpec, PlaybackStream, StableDeviceKey, StreamRequest,
};
use relay_core::{AudioGeneration, SystemEvent};
use relay_engine::media::{
	DeviceCmd, DeviceSetup, DeviceThread, EventSink, Inbox, MediaEvent, Selection,
};
use relay_engine::service::ports::{DeviceDirection, DeviceInfo};
use relay_engine::service::usage::device_use;

const MIC: &str = "Built-in Microphone";
const USB: &str = "USB Microphone";
const HEADSET: &str = "Headset Microphone";
const SPEAKERS: &str = "Built-in Speakers";
const PHONES: &str = "Headphones";
const MONITOR: &str = "Monitor";

/// The shared, test-controlled view of the system: which device is the
/// default per direction, and what was opened.
#[derive(Default)]
struct World {
	default_in: String,
	default_out: String,
	opened: Vec<String>,
}

/// A fake backend whose system default can be changed while it runs.
struct Scripted {
	inner: FakeBackend,
	world: Arc<Mutex<World>>,
	last: Option<Enumeration>,
}

impl Scripted {
	fn name_of(&self, device: DeviceHandle) -> String {
		self.last
			.as_ref()
			.and_then(|e| e.get(device))
			.map(|d| d.key.name.clone())
			.unwrap_or_default()
	}
}

impl AudioBackend for Scripted {
	fn enumerate(&mut self) -> relay_audio::Result<Enumeration> {
		let mut enumeration = self.inner.enumerate()?;
		let world = self.world.lock().unwrap();
		for device in &mut enumeration.devices {
			let wanted = match device.key.direction {
				Direction::Input => &world.default_in,
				Direction::Output => &world.default_out,
			};
			device.is_default = &device.key.name == wanted;
		}
		self.last = Some(enumeration.clone());
		Ok(enumeration)
	}

	fn open_capture(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> relay_audio::Result<CaptureStream> {
		let name = self.name_of(device);
		self.world.lock().unwrap().opened.push(name);
		self.inner.open_capture(device, request)
	}

	fn open_playback(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> relay_audio::Result<PlaybackStream> {
		let name = self.name_of(device);
		self.world.lock().unwrap().opened.push(name);
		self.inner.open_playback(device, request)
	}
}

struct Rig {
	thread: DeviceThread,
	world: Arc<Mutex<World>>,
	events: Arc<Mutex<Vec<MediaEvent>>>,
}

fn specs() -> Vec<FakeDeviceSpec> {
	let mic = |name| FakeDeviceSpec::new(name, Direction::Input);
	let out = |name| FakeDeviceSpec::new(name, Direction::Output);
	vec![
		mic(MIC),
		mic(USB),
		mic(HEADSET),
		out(SPEAKERS),
		out(PHONES),
		out(MONITOR),
	]
}

fn start(selection: Selection) -> Rig {
	let world = Arc::new(Mutex::new(World {
		default_in: MIC.to_owned(),
		default_out: SPEAKERS.to_owned(),
		opened: Vec::new(),
	}));
	let events = Arc::new(Mutex::new(Vec::new()));
	let sink: EventSink = {
		let events = Arc::clone(&events);
		Arc::new(move |event| events.lock().unwrap().push(event))
	};
	let (capture, _capture_rx) = Inbox::new(16);
	let (render, _render_rx) = Inbox::new(16);
	let backend_world = Arc::clone(&world);
	let thread = DeviceThread::spawn(
		Box::new(move || {
			Box::new(Scripted {
				inner: FakeBackend::new(specs()),
				world: backend_world,
				last: None,
			})
		}),
		DeviceSetup {
			request: StreamRequest::default(),
			selection,
			playout_target_ms: 20,
		},
		capture,
		render,
		sink,
	)
	.unwrap();
	std::mem::forget((_capture_rx, _render_rx));
	Rig {
		thread,
		world,
		events,
	}
}

impl Rig {
	fn open_mic(&self, generation: u64) -> bool {
		let generation = AudioGeneration::new(generation);
		self.thread
			.commands
			.send(DeviceCmd::OpenCapture(generation))
			.unwrap();
		wait(|| {
			self.events.lock().unwrap().iter().any(|e| {
				matches!(
					e,
					MediaEvent::System(SystemEvent::CaptureReady { generation: g }
						| SystemEvent::CaptureFailed { generation: g }) if *g == generation
				)
			})
		});
		self.events.lock().unwrap().iter().any(|e| {
			matches!(
				e,
				MediaEvent::System(SystemEvent::CaptureReady { generation: g }) if *g == generation
			)
		})
	}

	fn open_speakers(&self) -> bool {
		let (reply, done) = tokio::sync::oneshot::channel();
		self.thread
			.commands
			.send(DeviceCmd::OpenPlayback(reply))
			.unwrap();
		done.blocking_recv().unwrap().is_ok()
	}

	fn opened(&self) -> Vec<String> {
		self.world.lock().unwrap().opened.clone()
	}

	fn key(&self, name: &str) -> StableDeviceKey {
		wait(|| !self.thread.listing.load().is_empty());
		let listing = self.thread.listing.load();
		listing
			.iter()
			.find(|d| d.key.name == name)
			.map(|d| d.key.clone())
			.unwrap_or_else(|| StableDeviceKey {
				host: "fake".to_owned(),
				direction: Direction::Input,
				backend_id: Some(format!("fake:{name}")),
				name: name.to_owned(),
				manufacturer: None,
				interface: None,
			})
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
fn starts_on_the_system_defaults_without_any_selection() {
	let rig = start(Selection::default());
	assert!(rig.open_mic(1));
	assert!(rig.open_speakers());
	assert_eq!(rig.opened(), [MIC, SPEAKERS]);
}

#[test]
fn a_pinned_device_wins_over_the_default() {
	let rig = start(Selection::default());
	let usb = rig.key(USB);
	let phones = rig.key(PHONES);
	rig.thread
		.commands
		.send(DeviceCmd::Select(Box::new(Selection {
			input: DeviceSelection::Pinned(usb),
			output: DeviceSelection::Pinned(phones),
		})))
		.unwrap();
	assert!(rig.open_mic(1));
	assert!(rig.open_speakers());
	assert_eq!(rig.opened(), [USB, PHONES]);
}

#[test]
fn a_vanished_pinned_device_falls_back_to_the_default() {
	let gone = StableDeviceKey {
		host: "fake".to_owned(),
		direction: Direction::Input,
		backend_id: Some("fake:Unplugged Microphone".to_owned()),
		name: "Unplugged Microphone".to_owned(),
		manufacturer: None,
		interface: None,
	};
	let rig = start(Selection {
		input: DeviceSelection::Pinned(gone.clone()),
		output: DeviceSelection::FollowDefault,
	});
	assert!(rig.open_mic(1));
	assert_eq!(rig.opened(), [MIC]);

	// The state reports it: pinned, missing, and the default is in use.
	let devices: BTreeMap<String, DeviceInfo> = rig
		.thread
		.listing
		.load()
		.iter()
		.map(|d| {
			let id = relay_engine::media::device_id(d);
			let info = DeviceInfo {
				id: id.clone(),
				name: d.key.name.clone(),
				direction: match d.key.direction {
					Direction::Input => DeviceDirection::Input,
					Direction::Output => DeviceDirection::Output,
				},
				available: true,
				system_default: d.is_default,
				rates: vec![48_000],
			};
			(id, info)
		})
		.collect();
	let used = device_use(&devices, "in:fake:fake:Unplugged Microphone", "");
	assert!(used.input.pinned_missing);
	assert!(!used.input.is_default);
	assert_eq!(used.input.name, MIC);
	assert!(used.output.is_default);
	assert_eq!(used.output.name, SPEAKERS);
	let pinned = device_use(&devices, "in:fake:fake:USB Microphone", "");
	assert!(!pinned.input.pinned_missing);
	assert_eq!(pinned.input.name, USB);
}

#[test]
fn a_changed_default_is_picked_up_by_the_rescan_and_the_next_open() {
	let rig = start(Selection::default());
	assert!(rig.open_mic(1));
	assert_eq!(rig.opened(), [MIC]);

	// A headset becomes the system default while the microphone is open.
	rig.world.lock().unwrap().default_in = HEADSET.to_owned();
	wait(|| {
		rig.thread
			.listing
			.load()
			.iter()
			.any(|d| d.key.name == HEADSET && d.is_default)
	});
	// The open stream is not restarted behind the call's back.
	assert_eq!(rig.opened(), [MIC]);

	// The next open (the next call) follows the new default.
	assert!(rig.open_mic(2));
	assert_eq!(rig.opened(), [MIC, HEADSET]);
}
