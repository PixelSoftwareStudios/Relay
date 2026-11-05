use relay_audio::AudioError;
use relay_audio::device::{
	DeviceSelection, Direction, FakeBackend, FakeDeviceSpec, Recovery, SampleFormatKind, Signal,
};
use relay_audio::pipeline::{LoopbackConfig, LoopbackPipeline};

fn devices(rate: u32) -> FakeBackend {
	FakeBackend::new(vec![
		FakeDeviceSpec {
			is_default: true,
			sample_rate: rate,
			format: SampleFormatKind::I16,
			..FakeDeviceSpec::new("mic", Direction::Input)
		},
		FakeDeviceSpec {
			is_default: true,
			channels: 2,
			sample_rate: rate,
			format: SampleFormatKind::F32,
			..FakeDeviceSpec::new("out", Direction::Output)
		},
	])
}

fn run(
	rate: u32,
	loss: Option<relay_audio::pipeline::LossFn>,
) -> (Vec<f32>, relay_audio::pipeline::StepStats) {
	let mut backend = devices(rate);
	let mut config = LoopbackConfig::new(
		DeviceSelection::FollowDefault,
		DeviceSelection::FollowDefault,
	);
	config.loss = loss;
	let mut pipe = LoopbackPipeline::open(&mut backend, config).unwrap();
	let mut mic = backend.take_capture_driver().unwrap();
	let mut out = backend.take_playback_driver().unwrap();
	mic.set_signal(Signal::Sine {
		freq_hz: 500.0,
		amplitude: 0.5,
	});
	out.enable_recording(rate as usize * 2);
	let block = (rate / 100) as usize;
	for _ in 0..150 {
		mic.run(block);
		pipe.step().unwrap();
		out.render(block);
	}
	assert_eq!(pipe.recovery(), Recovery::Continue);
	(out.recorded().to_vec(), pipe.stats())
}

fn rms(s: &[f32]) -> f32 {
	(s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
}

#[test]
fn audio_survives_the_whole_chain_at_48k_and_44k1() {
	for rate in [48_000, 44_100] {
		let (recorded, stats) = run(rate, None);
		assert!(stats.frames_encoded >= 140, "rate {rate}: {stats:?}");
		assert_eq!(stats.frames_encoded, stats.frames_decoded);
		let tail = &recorded[recorded.len() / 2..];
		let level = rms(tail);
		assert!(
			(level - 0.5 / 2.0_f32.sqrt()).abs() < 0.08,
			"rate {rate} rms {level}"
		);
	}
}

#[test]
fn lost_packets_are_concealed_not_skipped() {
	let (_, stats) = run(48_000, Some(Box::new(|seq| seq % 10 == 5)));
	assert!(stats.frames_concealed >= 10);
	assert_eq!(
		stats.frames_concealed + stats.frames_decoded,
		stats.frames_encoded
	);
}

#[test]
fn device_removal_pauses_without_fallback() {
	let mut backend = devices(48_000);
	let mut pipe = LoopbackPipeline::open(
		&mut backend,
		LoopbackConfig::new(
			DeviceSelection::FollowDefault,
			DeviceSelection::FollowDefault,
		),
	)
	.unwrap();
	let mut mic = backend.take_capture_driver().unwrap();
	mic.set_signal(Signal::Sine {
		freq_hz: 300.0,
		amplitude: 0.3,
	});
	mic.run(480);
	pipe.step().unwrap();
	let before = pipe.stats().frames_captured;
	mic.unplug();
	mic.run(480);
	assert_eq!(pipe.recovery(), Recovery::PauseAndPrompt);
	assert_eq!(pipe.step().unwrap().frames_captured, before);
}

#[test]
fn missing_pinned_device_fails_to_open() {
	let mut backend = devices(48_000);
	let e = relay_audio::device::AudioBackend::enumerate(&mut backend).unwrap();
	let mut key = e.of_direction(Direction::Input).next().unwrap().key.clone();
	key.backend_id = Some("gone".into());
	let result = LoopbackPipeline::open(
		&mut backend,
		LoopbackConfig::new(DeviceSelection::Pinned(key), DeviceSelection::FollowDefault),
	);
	assert!(matches!(result, Err(AudioError::SelectedDeviceMissing)));
}
