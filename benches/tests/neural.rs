//! Neural candidates. Tests marked ignored need the local weights in
//! `benches/models` (see `docs/evidence.md`); the rest run anywhere.
#![cfg(feature = "neural")]

use relay_audio::dsp::ModelManifest;
use relay_benches::audio::neural::gtcrn::config;
use relay_benches::audio::neural::spec::SpecModel;
use relay_benches::audio::neural::{NAMES, adapter::HopModel, make, make_in, models_dir};
use relay_benches::audio::wav::read_mono;

#[test]
fn every_committed_manifest_parses_and_is_a_candidate() {
	for name in NAMES {
		let text =
			std::fs::read_to_string(models_dir().join("manifests").join(format!("{name}.json")))
				.unwrap();
		let manifest = ModelManifest::from_json(&text).unwrap();
		assert_eq!(manifest.backend_id, name);
		assert!(!manifest.distribution.approved);
		assert!(manifest.source.revision.is_some() && manifest.model.sha256.is_some());
	}
}

#[test]
fn tampered_weights_are_refused() {
	let dir = tempfile::tempdir().unwrap();
	std::fs::create_dir_all(dir.path().join("manifests")).unwrap();
	std::fs::create_dir_all(dir.path().join("gtcrn")).unwrap();
	std::fs::copy(
		models_dir().join("manifests/gtcrn.json"),
		dir.path().join("manifests/gtcrn.json"),
	)
	.unwrap();
	std::fs::write(
		dir.path()
			.join("gtcrn/stream_onnx_models_gtcrn_simple.onnx"),
		b"not the model",
	)
	.unwrap();
	let error = make_in("gtcrn", dir.path())
		.err()
		.expect("must refuse")
		.to_string();
	assert!(error.contains("does not match the manifest"), "{error}");
}

fn snr_db(reference: &[f32], estimate: &[f32]) -> f64 {
	let signal: f64 = reference.iter().map(|&s| f64::from(s).powi(2)).sum();
	let error: f64 = reference
		.iter()
		.zip(estimate)
		.map(|(&a, &b)| f64::from(a - b).powi(2))
		.sum();
	10.0 * (signal / error).log10()
}

fn stream(model: &mut dyn HopModel, input: &[f32]) -> Vec<f32> {
	let hop = model.hop();
	let mut out = Vec::new();
	let mut block = vec![0.0_f32; hop];
	for chunk in input.chunks_exact(hop) {
		model.step(chunk, &mut block).unwrap();
		out.extend_from_slice(&block);
	}
	out
}

/// Best integer lag of `ours` against `reference` (within +/-1500) and the
/// SNR of `ours` against the reference at that lag, skipping the first
/// second.
fn agreement(reference: &[f32], ours: &[f32], rate: usize) -> (i64, f64) {
	let skip = rate;
	let n = ours.len().min(reference.len()) - 1600;
	let mut best = (0_i64, f64::MIN);
	for lag in -1500_i64..=1500 {
		let dot: f64 = (skip..n)
			.map(|i| f64::from(reference[i]) * f64::from(ours[(i as i64 + lag) as usize]))
			.sum();
		if dot > best.1 {
			best = (lag, dot);
		}
	}
	let at = |i: usize| ours[(i as i64 + best.0) as usize];
	let aligned: Vec<f32> = (skip..n).map(at).collect();
	(best.0, snr_db(&reference[skip..n], &aligned))
}

fn local(sub: &str) -> std::path::PathBuf {
	models_dir().join(sub)
}

/// The repository ships a noisy clip and its output from the reference
/// ONNX Runtime streaming script; ours must reproduce it.
#[test]
#[ignore = "needs the local GTCRN weights and test wavs"]
fn gtcrn_matches_the_reference_streaming_output() {
	let bytes = std::fs::read(local("gtcrn/stream_onnx_models_gtcrn_simple.onnx")).unwrap();
	let (mix, rate) = read_mono(&local("gtcrn/stream_test_mix.wav")).unwrap();
	let (reference, _) = read_mono(&local("gtcrn/stream_test_enh_stream.wav")).unwrap();
	assert_eq!(rate, 16_000);
	let mut model = SpecModel::new(config(), &bytes).unwrap();
	// The reference pads half a window at the start and has no delay; ours
	// emits each sample n_fft - hop = 256 samples late on top of that.
	let mut padded = vec![0.0_f32; 256];
	padded.extend_from_slice(&mix);
	let (lag, snr) = agreement(&reference, &stream(&mut model, &padded), 16_000);
	println!("gtcrn vs repository reference output: lag {lag}, {snr:.1} dB");
	assert!(snr > 40.0, "{snr}");
}

/// Reference outputs produced with ONNX Runtime 1.31.1 (see
/// `docs/evidence.md`) from the same weights and the same clip.
#[test]
#[ignore = "needs the local weights and ONNX Runtime reference wavs"]
fn dtln_and_fastenhancer_match_onnx_runtime() {
	let (mix, _) = read_mono(&local("gtcrn/stream_test_mix.wav")).unwrap();
	let (first, second) = (
		std::fs::read(local("dtln/pretrained_model_model_1.onnx")).unwrap(),
		std::fs::read(local("dtln/pretrained_model_model_2.onnx")).unwrap(),
	);
	let mut dtln = relay_benches::audio::neural::dtln::Dtln::from_bytes(&first, &second).unwrap();
	let (reference, _) = read_mono(&local("dtln/ref_out.wav")).unwrap();
	let (lag, snr) = agreement(&reference, &stream(&mut dtln, &mix), 16_000);
	println!("dtln vs onnxruntime: lag {lag}, {snr:.1} dB");
	assert!(lag == 0 && snr > 40.0);

	let (mix48, _) = read_mono(&local("fastenhancer/ref_mix48.wav")).unwrap();
	for (size, hop, caches) in [("t", 512, 2), ("s", 512, 3), ("m", 320, 4), ("l", 200, 5)] {
		let width = match size {
			"t" => [24, 20],
			"s" => [48, 48],
			"m" => [72, 72],
			_ => [96, 96],
		};
		let bytes = std::fs::read(local(&format!(
			"fastenhancer/fastenhancer_{size}_spec.onnx"
		)))
		.unwrap();
		let config = relay_benches::audio::neural::spec::SpecConfig {
			id: "fastenhancer",
			n_fft: 1024,
			hop,
			window: relay_benches::audio::neural::spec::Window::Hann,
			caches: (0..caches).map(|_| vec![1, width[0], width[1]]).collect(),
		};
		let mut model = SpecModel::new(config, &bytes).unwrap();
		// The reference pads n_fft/2 on both sides and has no delay; the
		// streaming model emits n_fft - hop samples late.
		let mut padded = vec![0.0_f32; 512];
		padded.extend_from_slice(&mix48);
		let (reference, _) =
			read_mono(&local(&format!("fastenhancer/ref_out_{size}.wav"))).unwrap();
		let (lag, snr) = agreement(&reference, &stream(&mut model, &padded), 48_000);
		println!("fastenhancer-{size} vs onnxruntime: lag {lag}, {snr:.1} dB");
		assert!(snr > 20.0, "{size}: lag {lag}, {snr}");
	}
}

#[test]
#[ignore = "needs the local weights"]
fn every_model_is_gapless_and_finite_on_noise() {
	for name in NAMES {
		let mut ns = make(name).unwrap();
		let mut state = 1_u32;
		let mut out = vec![0.0_f32; 960];
		for n in 0..300_u64 {
			let frame: Vec<f32> = (0..480)
				.map(|_| {
					state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
					(state >> 8) as f32 / (1_u32 << 24) as f32 * 0.2 - 0.1
				})
				.collect();
			let timing = relay_audio::format::BlockTiming {
				generation: 0,
				first_sample: n * 480,
			};
			let p = ns.process_into(&frame, &mut out, timing).unwrap();
			assert_eq!(p.produced, 480, "{name}");
			assert!(out[..480].iter().all(|s| s.is_finite()), "{name}");
		}
	}
}
