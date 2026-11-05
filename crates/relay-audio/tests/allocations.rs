//! Proves the steady-state callback and processing paths never touch the heap.
//!
//! A global allocator counts allocations made on the current thread while a
//! guard is armed. Setup (including warm-up) is allowed to allocate; the
//! measured regions are not.

#[path = "common/counting.rs"]
mod counting;

use relay_audio::capture::{CaptureChain, CaptureSetup, capture_pair};
use relay_audio::codec::{CallDecoder, CallEncoder, EncoderConfig, MAX_PACKET_BYTES};
use relay_audio::convert::{ChannelSelect, StreamResampler};
use relay_audio::device::{
	DeviceSelection, Direction, FakeBackend, FakeDeviceSpec, SampleFormatKind, Signal,
};
use relay_audio::mixer::Mixer;
use relay_audio::pipeline::{LoopbackConfig, LoopbackPipeline};
use relay_audio::playback::playback_pair;

use counting::allocations_in;

#[test]
fn counter_actually_counts() {
	let n = allocations_in(|| {
		let v = std::hint::black_box(vec![1_u8; 64]);
		drop(v);
	});
	assert!(n >= 1, "the counting allocator must observe allocations");
}

#[test]
fn capture_callback_is_allocation_free() {
	let (mut cb, mut reader, shared) = capture_pair(CaptureSetup {
		channels: 2,
		select: ChannelSelect::Average,
		ring_samples: 4096,
	});
	let f32_data = vec![0.1_f32; 2 * 480];
	let i16_data = vec![1000_i16; 2 * 480];
	let mut sink = vec![0.0_f32; 1024];
	let n = allocations_in(|| {
		for i in 0..200 {
			cb.process(&f32_data);
			cb.process(&i16_data);
			if i == 50 {
				shared.set_muted(true);
			}
			if i == 60 {
				shared.set_muted(false);
			}
			// Includes overrun and gap-event paths: the ring is small.
			if i % 3 == 0 {
				reader.read(&mut sink);
			}
		}
	});
	assert_eq!(n, 0);
}

#[test]
fn playback_callback_is_allocation_free() {
	let (mut cb, mut writer, shared) = playback_pair(2, 4096);
	let mut f32_out = vec![0.0_f32; 2 * 480];
	let mut i16_out = vec![0_i16; 2 * 480];
	let data = vec![0.2_f32; 300];
	let n = allocations_in(|| {
		for i in 0..200 {
			writer.write(&data);
			cb.process(&mut f32_out); // underruns and normal paths
			cb.process(&mut i16_out);
			if i == 100 {
				shared.request_flush();
			}
		}
	});
	assert_eq!(n, 0);
	assert!(shared.underrun_frames() > 0);
}

#[test]
fn resampler_codec_and_mixer_are_allocation_free() {
	let mut resampler = StreamResampler::new(44_100, 48_000, 441).unwrap();
	let mut enc = CallEncoder::new(EncoderConfig::default()).unwrap();
	let mut dec = CallDecoder::new().unwrap();
	let mut mixer = Mixer::new(480);
	let slot = mixer.open_slot().unwrap();
	let input: Vec<f32> = (0..441).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
	let frame: Vec<f32> = (0..480).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
	let mut packet = [0u8; MAX_PACKET_BYTES];
	let mut decoded = [0.0_f32; 480];
	let mut mixed = [0.0_f32; 480];
	let mut produced = 0;
	// Warm-up: first calls may initialise lazily inside the libraries.
	for _ in 0..20 {
		resampler.push(&input, |b| produced += b.len()).unwrap();
		let len = enc.encode(&frame, &mut packet).unwrap();
		dec.decode(&packet[..len], &mut decoded).unwrap();
	}
	let n = allocations_in(|| {
		for _ in 0..100 {
			resampler
				.push(&input[..300], |b| produced += b.len())
				.unwrap();
			resampler
				.push(&input[300..], |b| produced += b.len())
				.unwrap();
			resampler.set_drift_ppm(50.0).unwrap();
			let len = enc.encode(&frame, &mut packet).unwrap();
			dec.decode(&packet[..len], &mut decoded).unwrap();
			dec.conceal(&mut decoded).unwrap();
			mixer.begin();
			mixer.add(slot, &decoded).unwrap();
			mixer.finish(&mut mixed).unwrap();
		}
	});
	assert!(produced > 0);
	assert_eq!(n, 0);
}

#[test]
fn full_loopback_step_is_allocation_free() {
	let mut backend = FakeBackend::new(vec![
		FakeDeviceSpec {
			is_default: true,
			sample_rate: 44_100,
			channels: 2,
			format: SampleFormatKind::I16,
			..FakeDeviceSpec::new("mic", Direction::Input)
		},
		FakeDeviceSpec {
			is_default: true,
			sample_rate: 44_100,
			channels: 2,
			..FakeDeviceSpec::new("out", Direction::Output)
		},
	]);
	let mut config = LoopbackConfig::new(
		DeviceSelection::FollowDefault,
		DeviceSelection::FollowDefault,
	);
	config.loss = Some(Box::new(|seq| seq % 7 == 3));
	let mut pipe = LoopbackPipeline::open(&mut backend, config).unwrap();
	let mut mic = backend.take_capture_driver().unwrap();
	let mut out = backend.take_playback_driver().unwrap();
	mic.set_signal(Signal::Sine {
		freq_hz: 400.0,
		amplitude: 0.4,
	});
	for _ in 0..30 {
		mic.run(441);
		pipe.step().unwrap();
		out.render(441);
	}
	let n = allocations_in(|| {
		for _ in 0..300 {
			mic.run(441);
			pipe.step().unwrap();
			out.render(441);
		}
	});
	assert_eq!(n, 0, "steady-state device-to-device path must not allocate");
	assert!(pipe.stats().frames_encoded > 250);
}

#[test]
fn capture_chain_poll_is_allocation_free() {
	let mut backend = FakeBackend::new(vec![FakeDeviceSpec {
		is_default: true,
		sample_rate: 16_000,
		..FakeDeviceSpec::new("mic", Direction::Input)
	}]);
	let e = relay_audio::device::AudioBackend::enumerate(&mut backend).unwrap();
	let stream = relay_audio::device::AudioBackend::open_capture(
		&mut backend,
		e.devices[0].handle,
		&Default::default(),
	)
	.unwrap();
	let mut driver = backend.take_capture_driver().unwrap();
	driver.set_signal(Signal::Ramp);
	let mut chain = CaptureChain::new(stream.reader, 16_000).unwrap();
	let mut frames = 0;
	for _ in 0..10 {
		driver.run(160);
		chain.poll(|_, _| frames += 1).unwrap();
	}
	let n = allocations_in(|| {
		for _ in 0..200 {
			driver.run(160);
			chain.poll(|_, _| frames += 1).unwrap();
		}
	});
	assert_eq!(n, 0);
	assert!(frames > 100);
}

#[test]
fn dsp_chain_is_allocation_free() {
	use relay_audio::dsp::{DspChain, DspPreferences};
	use relay_audio::format::BlockTiming;
	let prefs = DspPreferences {
		auto_gain: true,
		limiter: true,
		..DspPreferences::default()
	};
	let mut chain = DspChain::new(prefs, None, None).unwrap();
	let mut frame: Vec<f32> = (0..480).map(|i| (i as f32 * 0.07).sin() * 0.05).collect();
	for _ in 0..10 {
		chain
			.process(&mut frame.clone(), BlockTiming::default())
			.unwrap();
	}
	let n = allocations_in(|| {
		for _ in 0..200 {
			chain.push_render(&frame).unwrap();
			chain.process(&mut frame, BlockTiming::default()).unwrap();
		}
	});
	assert_eq!(n, 0);
}

#[test]
fn drift_correction_at_48k_is_allocation_free() {
	let mut resampler = StreamResampler::new(48_000, 48_000, 480).unwrap();
	let input: Vec<f32> = (0..480).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
	let mut produced = 0;
	for _ in 0..20 {
		resampler.push(&input, |b| produced += b.len()).unwrap();
	}
	let n = allocations_in(|| {
		for i in 0..400 {
			let ppm = [300.0, -300.0, 100.0, -100.0][i % 4];
			resampler.set_drift_ppm(ppm).unwrap();
			resampler.push(&input, |b| produced += b.len()).unwrap();
		}
	});
	assert!(produced > 0);
	assert_eq!(n, 0);
}

#[test]
fn tone_render_is_allocation_free() {
	use relay_audio::tones::{Sound, TonePlayer};
	let (mut player, mut control) = TonePlayer::new();
	let mut mixer = Mixer::new(480);
	let mut out = [0.0_f32; 480];
	let n = allocations_in(|| {
		for i in 0..400 {
			match i % 100 {
				0 => control.start(Sound::Ring, true, 0.7).unwrap(),
				20 => control.start(Sound::Ringback, true, 0.7).unwrap(),
				40 => control.set_muted(true).unwrap(),
				50 => control.set_muted(false).unwrap(),
				60 => control.start(Sound::CallEnd, false, 1.0).unwrap(),
				80 => control.stop().unwrap(),
				_ => {}
			}
			mixer.begin();
			mixer.add_tones(&mut player);
			mixer.finish(&mut out).unwrap();
		}
	});
	assert_eq!(n, 0);
}
