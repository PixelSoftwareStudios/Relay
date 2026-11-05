//! Evaluation-candidate checks; compiled only with the `eval-*` features.
#![cfg(any(feature = "eval-sonora", feature = "eval-nnnoiseless"))]

#[path = "common/counting.rs"]
mod counting;

use counting::allocations_in;
use relay_audio::dsp::{DspLimits, NoiseSuppressor};
use relay_audio::format::BlockTiming;

const LIMITS: DspLimits = DspLimits {
	max_block_samples: 480,
	max_scratch_bytes: 0,
};

fn noise_frame(seed: &mut u32) -> Vec<f32> {
	(0..480)
		.map(|_| {
			*seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
			((*seed >> 8) as f32 / (1 << 24) as f32 - 0.5) * 0.1
		})
		.collect()
}

fn energy(s: &[f32]) -> f32 {
	s.iter().map(|v| v * v).sum()
}

/// Steady-state noise must be attenuated and stay finite. Returns the
/// allocations observed over 100 steady-state frames (a measurement, not a
/// requirement: candidates are evaluated, not assumed real-time safe).
fn check_candidate(mut ns: impl NoiseSuppressor) -> u64 {
	ns.prepare(&LIMITS).unwrap();
	let mut seed = 7;
	let mut out = vec![0.0; 480];
	let (mut e_in, mut e_out) = (0.0, 0.0);
	for n in 0..300 {
		let frame = noise_frame(&mut seed);
		let t = BlockTiming {
			generation: 0,
			first_sample: n * 480,
		};
		let p = ns.process_into(&frame, &mut out, t).unwrap();
		assert_eq!(p.produced, 480);
		assert!(out.iter().all(|v| v.is_finite()));
		if n >= 200 {
			e_in += energy(&frame);
			e_out += energy(&out);
		}
	}
	assert!(
		e_out < e_in * 0.8,
		"noise should be reduced: {e_out} vs {e_in}"
	);
	let frame = noise_frame(&mut seed);
	allocations_in(|| {
		for n in 0..100 {
			let t = BlockTiming {
				generation: 0,
				first_sample: n * 480,
			};
			ns.process_into(&frame, &mut out, t).unwrap();
		}
	})
}

#[cfg(feature = "eval-sonora")]
mod sonora {
	use super::*;
	use relay_audio::dsp::eval::{SonoraProfile, SonoraSuppressor};
	use relay_audio::dsp::{ChainError, DspChain, DspPreferences, StageKind};

	#[test]
	fn ns_only_attenuates() {
		let allocs = check_candidate(SonoraSuppressor::new(SonoraProfile::NsOnly));
		eprintln!("sonora-ns allocations over 100 frames: {allocs}");
	}

	#[test]
	fn ns_agc_profile_runs() {
		let allocs = check_candidate(SonoraSuppressor::new(SonoraProfile::NsAgc));
		eprintln!("sonora-ns-agc allocations over 100 frames: {allocs}");
	}

	#[test]
	fn ns_aec_profile_accepts_render_reference() {
		let mut ns = SonoraSuppressor::new(SonoraProfile::NsAec);
		ns.prepare(&LIMITS).unwrap();
		let render = vec![0.1; 480];
		let mut out = vec![0.0; 480];
		let allocs_ok = {
			for _ in 0..50 {
				ns.push_render(&render).unwrap();
				ns.process_into(&render, &mut out, BlockTiming::default())
					.unwrap();
			}
			allocations_in(|| {
				for _ in 0..50 {
					ns.push_render(&render).unwrap();
					ns.process_into(&render, &mut out, BlockTiming::default())
						.unwrap();
				}
			})
		};
		eprintln!("sonora-ns-aec allocations over 50 frames (capture+render): {allocs_ok}");
		assert!(ns.push_render(&render[..100]).is_err());
	}

	#[test]
	fn bundled_agc_conflicts_with_chain_agc() {
		let prefs = DspPreferences {
			noise_suppression: true,
			auto_gain: true,
			..DspPreferences::default()
		};
		let ns = SonoraSuppressor::new(SonoraProfile::NsAgc);
		assert!(ns.descriptor().provides.contains(StageKind::AutoGain));
		assert!(matches!(
			DspChain::new(prefs, Some(Box::new(ns)), None),
			Err(ChainError::DuplicateStage(StageKind::AutoGain))
		));
		let ns_only = SonoraSuppressor::new(SonoraProfile::NsOnly);
		let chain = DspChain::new(prefs, Some(Box::new(ns_only)), None).unwrap();
		assert!(chain.provides().contains(StageKind::NoiseSuppress));
	}
}

#[cfg(feature = "eval-nnnoiseless")]
#[test]
fn nnnoiseless_attenuates_without_allocating() {
	let allocs = check_candidate(relay_audio::dsp::eval::NnnoiselessSuppressor::new());
	assert_eq!(
		allocs, 0,
		"nnnoiseless per-frame processing must not allocate"
	);
}
