//! The product processing stage: bypass, framing, suppression, gain, delay and overrun.

use std::time::{Duration, Instant};

use relay_audio::capture::{CaptureChain, CaptureSetup, capture_pair};
use relay_audio::convert::ChannelSelect;
use relay_audio::dsp::{
	Discontinuity, DspError, DspLimits, NoiseLevel, NoiseSuppressor, ProcessStatus, SonoraConfig,
	SonoraSuppressor, StageKind, create_processor, measure_delay_samples,
};
use relay_audio::format::{BlockTiming, FRAME_SAMPLES};

const LIMITS: DspLimits = DspLimits {
	max_block_samples: FRAME_SAMPLES,
	max_scratch_bytes: 0,
};

const fn timing(n: usize) -> BlockTiming {
	BlockTiming {
		generation: 0,
		first_sample: (n * FRAME_SAMPLES) as u64,
	}
}

struct Noise(u32);

impl Noise {
	fn next(&mut self, amplitude: f32) -> f32 {
		self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
		((self.0 >> 8) as f32 / (1 << 24) as f32 - 0.5) * 2.0 * amplitude
	}
}

fn energy(samples: &[f32]) -> f32 {
	samples.iter().map(|v| v * v).sum()
}

fn ns(level: NoiseLevel) -> SonoraSuppressor {
	let mut stage = SonoraSuppressor::new(SonoraConfig {
		noise_level: level,
		..SonoraConfig::default()
	});
	stage.prepare(&LIMITS).unwrap();
	stage
}

/// Runs `frames` of `source` through `stage`; returns (input, output) energy over the frames
/// from `from` on.
fn run(
	stage: &mut dyn NoiseSuppressor,
	frames: usize,
	from: usize,
	mut source: impl FnMut(usize) -> f32,
) -> (f32, f32) {
	let mut out = vec![0.0; FRAME_SAMPLES];
	let (mut e_in, mut e_out) = (0.0, 0.0);
	for n in 0..frames {
		let frame: Vec<f32> = (0..FRAME_SAMPLES)
			.map(|i| source(n * FRAME_SAMPLES + i))
			.collect();
		let progress = stage.process_into(&frame, &mut out, timing(n)).unwrap();
		assert_eq!(progress.produced, FRAME_SAMPLES);
		assert!(out.iter().all(|v| v.is_finite()));
		if n >= from {
			e_in += energy(&frame);
			e_out += energy(&out);
		}
	}
	(e_in, e_out)
}

#[test]
fn disabled_stage_is_the_plain_bit_exact_copy() {
	let config = SonoraConfig {
		noise_suppression: false,
		..SonoraConfig::default()
	};
	assert!(config.is_bypass());
	let mut stage = create_processor(config);
	assert_eq!(stage.descriptor().backend_id, "off");
	assert!(stage.descriptor().provides.is_empty());
	stage.prepare(&LIMITS).unwrap();
	let mut noise = Noise(3);
	let input: Vec<f32> = (0..FRAME_SAMPLES).map(|_| noise.next(0.9)).collect();
	let mut out = vec![0.0; FRAME_SAMPLES];
	let progress = stage.process_into(&input, &mut out, timing(0)).unwrap();
	assert_eq!(progress.produced, FRAME_SAMPLES);
	assert!(
		input
			.iter()
			.zip(&out)
			.all(|(a, b)| a.to_bits() == b.to_bits())
	);
	assert_eq!(
		measure_delay_samples(&mut *stage, FRAME_SAMPLES, 4_000).unwrap(),
		Some(0)
	);
}

#[test]
fn frames_are_ten_milliseconds_and_short_input_waits() {
	let mut stage = ns(NoiseLevel::Moderate);
	let mut out = vec![0.0; FRAME_SAMPLES];
	let short = stage
		.process_into(&[0.0; FRAME_SAMPLES - 1], &mut out, timing(0))
		.unwrap();
	assert_eq!((short.consumed, short.produced), (0, 0));
	assert_eq!(short.status, ProcessStatus::NeedMoreInput);
	let whole = stage
		.process_into(&[0.0; FRAME_SAMPLES], &mut out, timing(1))
		.unwrap();
	assert_eq!(
		(whole.consumed, whole.produced),
		(FRAME_SAMPLES, FRAME_SAMPLES)
	);
	assert_eq!(whole.first_output_index, timing(1).first_sample);
	assert!(matches!(
		stage.process_into(
			&[0.0; FRAME_SAMPLES],
			&mut out[..FRAME_SAMPLES - 1],
			timing(2)
		),
		Err(DspError::OutputTooSmall { .. })
	));
	assert!(stage.push_render(&[0.0; 10]).is_err());
}

#[test]
fn stages_are_reported_by_what_is_enabled() {
	let all = SonoraSuppressor::new(SonoraConfig {
		noise_suppression: true,
		noise_level: NoiseLevel::High,
		auto_gain: true,
		echo_cancel: true,
	});
	let provides = all.descriptor().provides;
	for kind in [
		StageKind::NoiseSuppress,
		StageKind::AutoGain,
		StageKind::EchoCancel,
	] {
		assert!(provides.contains(kind));
	}
	let only_ns = ns(NoiseLevel::Low);
	assert!(!only_ns.descriptor().provides.contains(StageKind::AutoGain));
	assert!(
		!only_ns
			.descriptor()
			.provides
			.contains(StageKind::EchoCancel)
	);
}

#[test]
fn steady_noise_under_a_tone_is_reduced_more_at_higher_levels() {
	let tone = |n: usize| 0.2 * (n as f32 * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0).sin();
	let mut gains = Vec::new();
	for level in [NoiseLevel::Low, NoiseLevel::High] {
		let mut noise = Noise(11);
		let mut stage = ns(level);
		// Noise alone, after the estimator has settled.
		let (e_in, e_out) = run(&mut stage, 400, 300, |_| noise.next(0.05));
		assert!(e_out < e_in * 0.9, "{level:?}: noise {e_out} vs {e_in}");
		gains.push(e_out / e_in);
		// A tone over the same noise must survive.
		let mut noise = Noise(11);
		let (t_in, t_out) = run(&mut stage, 200, 100, |n| tone(n) + noise.next(0.05));
		assert!(
			t_out > t_in * 0.2,
			"{level:?}: tone lost, {t_out} vs {t_in}"
		);
	}
	assert!(
		gains[1] < gains[0],
		"high should cut more than low: {gains:?}"
	);
}

#[test]
fn auto_gain_raises_a_quiet_voice_and_leaves_loud_audio_alone() {
	let mut stage = SonoraSuppressor::new(SonoraConfig {
		noise_suppression: false,
		auto_gain: true,
		..SonoraConfig::default()
	});
	stage.prepare(&LIMITS).unwrap();
	// Voiced-like: a 120 Hz glottal pulse train shaped by formants, in 250 ms syllables.
	let quiet = |n: usize| {
		let t = n as f32 / 48_000.0;
		let syllable = (t * 4.0).fract();
		let envelope = if syllable < 0.7 {
			(syllable / 0.7 * std::f32::consts::PI).sin()
		} else {
			0.0
		};
		let voiced: f32 = (1..40)
			.map(|h| {
				let f = 120.0 * h as f32;
				let formant = 1.0 / (1.0 + ((f - 700.0) / 300.0).powi(2))
					+ 0.6 / (1.0 + ((f - 1_800.0) / 400.0).powi(2));
				formant * (2.0 * std::f32::consts::PI * f * t).sin() / h as f32
			})
			.sum();
		0.01 * envelope * voiced
	};
	let (e_in, e_out) = run(&mut stage, 1_000, 800, quiet);
	assert!(e_out > e_in * 2.0, "gain should rise: {e_out} vs {e_in}");
	let loud = |n: usize| 0.9 * (n as f32 * 2.0 * std::f32::consts::PI * 300.0 / 48_000.0).sin();
	let mut out = vec![0.0; FRAME_SAMPLES];
	for n in 0..200 {
		let frame: Vec<f32> = (0..FRAME_SAMPLES)
			.map(|i| loud(n * FRAME_SAMPLES + i))
			.collect();
		stage.process_into(&frame, &mut out, timing(n)).unwrap();
		assert!(out.iter().all(|v| v.abs() <= 1.0));
	}
}

#[test]
fn the_declared_delay_matches_the_measured_one() {
	for noise_suppression in [true, false] {
		let mut stage = SonoraSuppressor::new(SonoraConfig {
			noise_suppression,
			auto_gain: true,
			..SonoraConfig::default()
		});
		check_delay(&mut stage);
	}
}

fn check_delay(stage: &mut SonoraSuppressor) {
	let declared = stage.descriptor().declared_delay_samples.unwrap();
	let mut noise = Noise(9);
	let (mut sent, mut got) = (Vec::new(), Vec::new());
	let mut out = vec![0.0; FRAME_SAMPLES];
	for n in 0..100 {
		let frame: Vec<f32> = (0..FRAME_SAMPLES).map(|_| noise.next(0.2)).collect();
		stage.process_into(&frame, &mut out, timing(n)).unwrap();
		sent.extend_from_slice(&frame);
		got.extend_from_slice(&out);
	}
	let score = |lag: usize| -> f32 {
		sent[..sent.len() - lag]
			.iter()
			.zip(&got[lag..])
			.skip(FRAME_SAMPLES * 20)
			.map(|(a, b)| a * b)
			.sum()
	};
	let measured = (0..FRAME_SAMPLES)
		.max_by(|a, b| score(*a).total_cmp(&score(*b)))
		.unwrap();
	assert!(
		measured.abs_diff(declared) <= 8,
		"declared {declared}, measured {measured}"
	);
}

#[test]
fn a_stalled_worker_never_blocks_the_callback_and_resumes_after_the_gap() {
	let (mut callback, reader, shared) = capture_pair(CaptureSetup {
		channels: 1,
		select: ChannelSelect::default(),
		ring_samples: 4_800,
	});
	let mut chain = CaptureChain::new(reader, 48_000).unwrap();
	let mut stage = ns(NoiseLevel::Moderate);
	// The worker does not run for a whole second of callbacks.
	let started = Instant::now();
	let mut noise = Noise(5);
	for _ in 0..100 {
		let block: Vec<f32> = (0..480).map(|_| noise.next(0.1)).collect();
		callback.process(&block);
	}
	assert!(started.elapsed() < Duration::from_secs(1));
	assert!(shared.overrun_frames() > 0);
	let mut out = vec![0.0; FRAME_SAMPLES];
	let mut frames = 0;
	let stats = chain
		.poll(|frame, at| {
			let progress = stage.process_into(frame, &mut out, at).unwrap();
			assert_eq!(progress.produced, FRAME_SAMPLES);
			frames += 1;
		})
		.unwrap();
	assert!(
		frames > 0 && frames <= 10,
		"only the ring survives: {frames}"
	);
	assert_eq!(stats.frames_emitted, frames);
	callback.process(&[0.0; 480]);
	let stats = chain.poll(|_, _| {}).unwrap();
	assert_eq!(stats.discontinuity, Some(Discontinuity::Overrun));
}
