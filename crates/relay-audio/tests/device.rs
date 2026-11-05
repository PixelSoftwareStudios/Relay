use relay_audio::AudioError;
use relay_audio::capture::CaptureChain;
use relay_audio::convert::ChannelSelect;
use relay_audio::device::{
	AudioBackend, DeviceSelection, Direction, FakeBackend, FakeDeviceSpec, Recovery, Resolution,
	Signal, StableDeviceKey, StreamRequest, StreamState, recovery_for,
};

fn mic(name: &str, default: bool) -> FakeDeviceSpec {
	FakeDeviceSpec {
		is_default: default,
		..FakeDeviceSpec::new(name, Direction::Input)
	}
}

fn backend() -> FakeBackend {
	FakeBackend::new(vec![
		mic("usb-headset", false),
		mic("laptop-mic", true),
		FakeDeviceSpec {
			is_default: true,
			..FakeDeviceSpec::new("speakers", Direction::Output)
		},
	])
}

#[test]
fn default_and_pinned_selection_resolve_distinctly() {
	let mut b = backend();
	let e = b.enumerate().unwrap();
	let Resolution::Device(h) = e.resolve(Direction::Input, &DeviceSelection::FollowDefault) else {
		panic!("default input expected");
	};
	assert_eq!(e.get(h).unwrap().key.name, "laptop-mic");
	let key = e.of_direction(Direction::Input).next().unwrap().key.clone();
	let Resolution::Device(h) = e.resolve(Direction::Input, &DeviceSelection::Pinned(key)) else {
		panic!("pinned input expected");
	};
	assert_eq!(e.get(h).unwrap().key.name, "usb-headset");
}

#[test]
fn missing_pinned_device_never_falls_back() {
	let mut b = backend();
	let e = b.enumerate().unwrap();
	let key = e.of_direction(Direction::Input).next().unwrap().key.clone();
	b.remove_device("usb-headset");
	let e2 = b.enumerate().unwrap();
	assert_eq!(
		e2.resolve(Direction::Input, &DeviceSelection::Pinned(key)),
		Resolution::PinnedMissing
	);
}

#[test]
fn stable_key_survives_serialisation_and_reenumeration() {
	let mut b = backend();
	let e = b.enumerate().unwrap();
	let key = e.of_direction(Direction::Input).next().unwrap().key.clone();
	let json = serde_json::to_string(&DeviceSelection::Pinned(key.clone())).unwrap();
	let back: DeviceSelection = serde_json::from_str(&json).unwrap();
	let e2 = b.enumerate().unwrap();
	assert_ne!(e.generation, e2.generation);
	assert!(matches!(
		e2.resolve(Direction::Input, &back),
		Resolution::Device(_)
	));
	let other = StableDeviceKey {
		backend_id: Some("different".into()),
		..key
	};
	assert!(matches!(
		e2.resolve(Direction::Input, &DeviceSelection::Pinned(other)),
		Resolution::PinnedMissing
	));
}

#[test]
fn stale_handles_are_rejected() {
	let mut b = backend();
	let old = b.enumerate().unwrap();
	let handle = old.of_direction(Direction::Input).next().unwrap().handle;
	let _new = b.enumerate().unwrap();
	assert!(matches!(
		b.open_capture(handle, &StreamRequest::default()),
		Err(AudioError::DeviceNotFound)
	));
	assert!(old.get(handle).is_some());
}

#[test]
fn capture_flows_through_chain_with_exact_indices() {
	let mut b = backend();
	let e = b.enumerate().unwrap();
	let h = e.of_direction(Direction::Input).next().unwrap().handle;
	let stream = b.open_capture(h, &StreamRequest::default()).unwrap();
	let mut driver = b.take_capture_driver().unwrap();
	driver.set_signal(Signal::Ramp);
	let mut chain = CaptureChain::new(stream.reader, stream.config.sample_rate).unwrap();
	let mut frames = Vec::new();
	for _ in 0..8 {
		driver.run(300); // deliberately not packet aligned
		chain.poll(|f, t| frames.push((t, f.to_vec()))).unwrap();
	}
	assert_eq!(frames.len(), 2400 / 480);
	for (n, (timing, frame)) in frames.iter().enumerate() {
		assert_eq!(timing.first_sample, (n * 480) as u64);
		assert_eq!(timing.generation, frames[0].0.generation);
		let i = n * 480;
		assert!((frame[0] - ((i % 1000) + 1) as f32 / 2000.0).abs() < 1e-6);
	}
}

#[test]
fn mute_starts_new_generation_and_keeps_source_timeline() {
	let mut b = backend();
	let e = b.enumerate().unwrap();
	let h = e.of_direction(Direction::Input).next().unwrap().handle;
	let stream = b.open_capture(h, &StreamRequest::default()).unwrap();
	let mut driver = b.take_capture_driver().unwrap();
	driver.set_signal(Signal::Ramp);
	let mut chain = CaptureChain::new(stream.reader, stream.config.sample_rate).unwrap();
	let mut frames = Vec::new();
	let mut run = |driver: &mut relay_audio::device::FakeCaptureDriver,
	               chain: &mut CaptureChain| {
		driver.run(480);
		chain.poll(|_, t| frames.push(t)).unwrap();
	};
	run(&mut driver, &mut chain);
	stream.shared.set_muted(true);
	run(&mut driver, &mut chain);
	run(&mut driver, &mut chain);
	stream.shared.set_muted(false);
	run(&mut driver, &mut chain);
	assert_eq!(frames.len(), 2);
	assert_eq!(frames[0].first_sample, 0);
	assert_eq!(
		frames[1].first_sample,
		3 * 480,
		"muted span stays in the timeline"
	);
	assert_ne!(frames[0].generation, frames[1].generation);
}

#[test]
fn channel_selection_picks_the_requested_channel() {
	let mut b = FakeBackend::new(vec![FakeDeviceSpec {
		channels: 2,
		is_default: true,
		..FakeDeviceSpec::new("stereo", Direction::Input)
	}]);
	let e = b.enumerate().unwrap();
	let h = e.devices[0].handle;
	let request = StreamRequest {
		select: ChannelSelect::Channel(1),
		..StreamRequest::default()
	};
	let mut stream = b.open_capture(h, &request).unwrap();
	let mut driver = b.take_capture_driver().unwrap();
	driver.set_signal(Signal::Impulse {
		at: 3,
		amplitude: 0.5,
	});
	driver.run(8);
	let mut out = [0.0; 8];
	let r = stream.reader.read(&mut out);
	assert_eq!(r.count, 8);
	assert_eq!(out[3], -0.5);
	let bad = StreamRequest {
		select: ChannelSelect::Channel(2),
		..StreamRequest::default()
	};
	assert!(b.open_capture(h, &bad).is_err());
}

#[test]
fn unplug_signals_recovery_and_stops_capture() {
	let mut b = backend();
	let e = b.enumerate().unwrap();
	let h = e.of_direction(Direction::Input).next().unwrap().handle;
	let stream = b.open_capture(h, &StreamRequest::default()).unwrap();
	let mut driver = b.take_capture_driver().unwrap();
	assert_eq!(stream.health.state(), StreamState::Running);
	driver.run(100);
	driver.unplug();
	driver.run(100);
	assert_eq!(stream.shared.frames_seen(), 100);
	assert_eq!(stream.health.state(), StreamState::DeviceLost);
	assert_eq!(
		recovery_for(Direction::Input, stream.health.state()),
		Recovery::PauseAndPrompt
	);
	// A later benign error never clears or downgrades the loss.
	stream.health.report(StreamState::Running);
	assert_eq!(stream.health.state(), StreamState::DeviceLost);
}

#[test]
#[ignore = "needs real audio hardware; run manually"]
fn real_hardware_enumeration_smoke() {
	let mut backend = relay_audio::device::CpalBackend::new();
	let e = backend.enumerate().unwrap();
	for d in &e.devices {
		eprintln!(
			"{:?} {} default={}",
			d.key.direction, d.key.name, d.is_default
		);
	}
}

#[test]
fn a_refused_realtime_promotion_does_not_pause_the_microphone() {
	// RT-15: cpal reports a failed promotion through the error callback; audio keeps flowing.
	use relay_audio::device::{StreamHealth, report_cpal_error};
	let health = StreamHealth::default();
	report_cpal_error(&health, &cpal::ErrorKind::RealtimeDenied.into());
	assert!(health.realtime_denied());
	assert_eq!(health.state(), StreamState::Running);
	assert_eq!(
		recovery_for(Direction::Input, health.state()),
		Recovery::Continue
	);
	report_cpal_error(&health, &cpal::ErrorKind::DeviceNotAvailable.into());
	assert_eq!(health.state(), StreamState::DeviceLost);
}

#[test]
fn playback_callback_publishes_its_largest_buffer() {
	// RT-06: the control side sizes the playout target from the observed callback.
	let (mut cb, _writer, shared) = relay_audio::playback::playback_pair(2, 8192);
	assert_eq!(shared.max_callback_frames(), 0);
	cb.process(&mut vec![0.0_f32; 2 * 480]);
	cb.process(&mut vec![0.0_f32; 2 * 2048]);
	cb.process(&mut vec![0.0_f32; 2 * 256]);
	assert_eq!(shared.max_callback_frames(), 2048);
}
