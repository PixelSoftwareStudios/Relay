use relay_audio::dsp::{
	Agc, AgcConfig, ChainError, DspChain, DspPreferences, EchoCanceller, HighPass, NoOpAec,
	NoiseSuppressor, OffSuppressor, Provides, StageKind,
};
use relay_audio::format::BlockTiming;

fn tone(freq: f32, amp: f32, n: usize) -> Vec<f32> {
	(0..n)
		.map(|i| (std::f32::consts::TAU * freq * i as f32 / 48_000.0).sin() * amp)
		.collect()
}

fn rms(s: &[f32]) -> f32 {
	(s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
}

#[test]
fn defaults_enable_only_the_high_pass() {
	let p = DspPreferences::default();
	assert!(p.high_pass);
	assert!(!p.echo_cancel && !p.noise_suppression && !p.auto_gain && !p.limiter);
	let chain = DspChain::new(p, None, None).unwrap();
	assert!(chain.provides().contains(StageKind::HighPass));
	assert!(!chain.provides().contains(StageKind::AutoGain));
}

#[test]
fn high_pass_removes_rumble_and_keeps_speech_band() {
	let mut hp = HighPass::new(80.0);
	let mut rumble = tone(20.0, 0.5, 4800);
	hp.process(&mut rumble);
	assert!(rms(&rumble[2400..]) < 0.5 / 2f32.sqrt() * 0.1);
	let mut hp = HighPass::new(80.0);
	let mut voice = tone(1000.0, 0.5, 4800);
	hp.process(&mut voice);
	assert!((rms(&voice[2400..]) - 0.5 / 2f32.sqrt()).abs() < 0.02);
	hp.process(&mut [f32::NAN, f32::INFINITY]);
}

fn run_agc(agc: &mut Agc, level: f32, frames: usize) -> Vec<f32> {
	let mut last = Vec::new();
	for _ in 0..frames {
		last = tone(300.0, level, 480);
		agc.process(&mut last);
	}
	last
}

#[test]
fn agc_raises_quiet_speech_within_limits() {
	let cfg = AgcConfig::default();
	let mut agc = Agc::new(cfg);
	run_agc(&mut agc, 0.02, 3000);
	assert!(agc.gain_db() > 3.0);
	assert!(agc.gain_db() <= cfg.max_gain_db + 1e-3, "gain is capped");
}

#[test]
fn agc_does_not_amplify_noise_below_the_gate() {
	let mut agc = Agc::new(AgcConfig::default());
	let out = run_agc(&mut agc, 0.001, 2000); // about -63 dBFS
	assert_eq!(agc.gain_db(), 0.0);
	assert!((rms(&out) - 0.001 / 2f32.sqrt()).abs() < 1e-5);
	// Gain earned on speech relaxes back toward unity in a long pause.
	run_agc(&mut agc, 0.02, 3000);
	assert!(agc.gain_db() > 1.0);
	run_agc(&mut agc, 0.001, 300);
	assert!(agc.gain_db() < 0.01 && agc.gain_db() > -0.01);
}

#[test]
fn agc_attenuates_loud_input_quickly() {
	let mut agc = Agc::new(AgcConfig::default());
	run_agc(&mut agc, 0.9, 100);
	assert!(agc.gain_db() < -5.0);
	assert!(agc.gain_db() >= AgcConfig::default().min_gain_db - 1e-3);
}

struct Dummy(Provides);

impl NoiseSuppressor for Dummy {
	fn descriptor(&self) -> &relay_audio::dsp::DspDescriptor {
		Box::leak(Box::new({
			let mut d = OffSuppressor::default().descriptor().clone();
			d.provides = self.0;
			d
		}))
	}
	fn prepare(
		&mut self,
		_: &relay_audio::dsp::DspLimits,
	) -> Result<(), relay_audio::dsp::DspError> {
		Ok(())
	}
	fn process_into(
		&mut self,
		input: &[f32],
		output: &mut [f32],
		t: BlockTiming,
	) -> Result<relay_audio::dsp::ProcessProgress, relay_audio::dsp::DspError> {
		OffSuppressor::default().process_into(input, output, t)
	}
	fn reset(&mut self, _: relay_audio::dsp::Discontinuity) {}
}

fn ns_with(p: Provides) -> Option<Box<dyn NoiseSuppressor>> {
	Some(Box::new(Dummy(p)))
}

#[test]
fn backend_that_bundles_agc_conflicts_with_a_separate_agc() {
	let bundled = Provides::of(StageKind::NoiseSuppress).with(StageKind::AutoGain);
	let prefs = DspPreferences {
		noise_suppression: true,
		auto_gain: true,
		..DspPreferences::default()
	};
	assert!(matches!(
		DspChain::new(prefs, ns_with(bundled), None),
		Err(ChainError::DuplicateStage(StageKind::AutoGain))
	));
	let ok = DspPreferences {
		auto_gain: false,
		..prefs
	};
	assert!(DspChain::new(ok, ns_with(bundled), None).is_ok());
}

#[test]
fn ns_must_be_provided_and_backends_must_match_the_enabled_stage() {
	let ns_on = DspPreferences {
		noise_suppression: true,
		..DspPreferences::default()
	};
	assert!(matches!(
		DspChain::new(ns_on, None, None),
		Err(ChainError::MissingBackend(StageKind::NoiseSuppress))
	));
	assert!(DspChain::new(ns_on, ns_with(Provides::NONE), None).is_err());
	let aec_on = DspPreferences {
		echo_cancel: true,
		..DspPreferences::default()
	};
	assert!(matches!(
		DspChain::new(aec_on, None, Some(Box::new(NoOpAec))),
		Err(ChainError::MissingBackend(StageKind::EchoCancel))
	));
}

#[test]
fn ns_backend_that_also_cancels_echo_conflicts_with_aec_stage() {
	struct FakeAec;
	impl EchoCanceller for FakeAec {
		fn provides(&self) -> Provides {
			Provides::of(StageKind::EchoCancel)
		}
		fn push_render(&mut self, _: &[f32]) -> Result<(), relay_audio::dsp::DspError> {
			Ok(())
		}
		fn process_capture(&mut self, _: &mut [f32]) -> Result<(), relay_audio::dsp::DspError> {
			Ok(())
		}
		fn reset(&mut self) {}
	}
	let prefs = DspPreferences {
		echo_cancel: true,
		noise_suppression: true,
		..DspPreferences::default()
	};
	let both = Provides::of(StageKind::NoiseSuppress).with(StageKind::EchoCancel);
	assert!(matches!(
		DspChain::new(prefs, ns_with(both), Some(Box::new(FakeAec))),
		Err(ChainError::DuplicateStage(StageKind::EchoCancel))
	));
}

#[test]
fn chain_rejects_wrong_frame_sizes_and_runs_in_place() {
	let prefs = DspPreferences {
		auto_gain: true,
		limiter: true,
		..DspPreferences::default()
	};
	let mut chain = DspChain::new(prefs, None, None).unwrap();
	assert!(matches!(
		chain.process(&mut [0.0; 100], BlockTiming::default()),
		Err(ChainError::BadFrame(100))
	));
	let mut frame = tone(300.0, 0.9, 480);
	for _ in 0..50 {
		frame = tone(300.0, 0.9, 480);
		chain.process(&mut frame, BlockTiming::default()).unwrap();
	}
	assert!(frame.iter().all(|v| v.abs() <= 1.0));
}

#[test]
fn agc_does_not_latch_to_nan_after_a_poisoned_frame() {
	// RT-12: one NaN from an upstream stage must not poison every later sample.
	let mut agc = Agc::new(AgcConfig::default());
	let mut bad = vec![0.1_f32; 480];
	bad[10] = f32::NAN;
	bad[11] = f32::INFINITY;
	agc.process(&mut bad);
	assert!(bad.iter().all(|s| s.is_finite()));
	assert!(agc.gain_db().is_finite());
	let mut good = vec![0.1_f32; 480];
	agc.process(&mut good);
	assert!(good.iter().all(|s| s.is_finite()));
}
