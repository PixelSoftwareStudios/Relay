use relay_benches::audio::stoi::stoi;
use relay_benches::audio::synth::{NoiseKind, mix_at_snr, noise, speech_like};

const RATE: u32 = 48_000;

fn clean() -> Vec<f32> {
	speech_like(5.0, RATE, 1)
}

#[test]
fn identical_signals_score_one() {
	let x = clean();
	let s = stoi(&x, &x, RATE).unwrap();
	assert!((s.stoi - 1.0).abs() < 1e-9, "stoi {}", s.stoi);
	assert!((s.estoi - 1.0).abs() < 1e-6, "estoi {}", s.estoi);
}

#[test]
fn level_changes_do_not_matter() {
	let x = clean();
	let y: Vec<f32> = x.iter().map(|v| v * 0.3).collect();
	let s = stoi(&x, &y, RATE).unwrap();
	assert!((s.stoi - 1.0).abs() < 1e-6, "stoi {}", s.stoi);
	assert!((s.estoi - 1.0).abs() < 1e-6, "estoi {}", s.estoi);
}

#[test]
fn appended_silence_is_removed() {
	let x = clean();
	let mut padded = x;
	padded.extend(std::iter::repeat_n(0.0, 96_000));
	let s = stoi(&padded, &padded, RATE).unwrap();
	assert!((s.stoi - 1.0).abs() < 1e-9);
}

#[test]
fn scores_fall_as_noise_rises() {
	let x = clean();
	let n = noise(NoiseKind::White, 5.0, RATE, 9);
	let scores: Vec<_> = [20.0, 10.0, 0.0, -10.0]
		.into_iter()
		.map(|snr| stoi(&x, &mix_at_snr(&x, &n, snr), RATE).unwrap())
		.collect();
	for pair in scores.windows(2) {
		assert!(pair[0].stoi > pair[1].stoi, "{scores:?}");
		assert!(pair[0].estoi > pair[1].estoi, "{scores:?}");
	}
	assert!(scores[0].stoi > 0.8, "{scores:?}");
	assert!(scores[3].stoi < scores[0].stoi - 0.2, "{scores:?}");
}

#[test]
fn unrelated_signal_scores_low() {
	let x = clean();
	let n = noise(NoiseKind::White, 5.0, RATE, 11);
	let s = stoi(&x, &n, RATE).unwrap();
	assert!(s.stoi < 0.4 && s.estoi < 0.4, "{s:?}");
}

#[test]
fn too_little_audio_has_no_score() {
	let x = speech_like(0.2, RATE, 1);
	assert!(stoi(&x, &x, RATE).is_none());
}
