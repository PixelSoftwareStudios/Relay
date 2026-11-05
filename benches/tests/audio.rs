use relay_benches::audio::corpus::{discover, generate_synthetic};
use relay_benches::audio::metrics::{align, correlation_delay, si_sdr_db};
use relay_benches::audio::runner::run;
use relay_benches::audio::synth::{NoiseKind, mix_at_snr, noise, rms, speech_like};
use relay_benches::audio::wav::{read_mono_48k, write_mono_48k};

#[test]
fn si_sdr_is_scale_invariant_and_detects_noise() {
	let clean = speech_like(2.0, 48_000, 1);
	let scaled: Vec<f32> = clean.iter().map(|v| v * 0.3).collect();
	let perfect = si_sdr_db(&clean, &scaled);
	assert!(perfect.is_none_or(|v| v > 50.0), "scaled copy: {perfect:?}");
	let noisy = mix_at_snr(&clean, &noise(NoiseKind::White, 2.0, 48_000, 3), 10.0);
	let sdr = si_sdr_db(&clean, &noisy).unwrap();
	assert!((sdr - 10.0).abs() < 0.7, "got {sdr}");
	let scaled_noisy: Vec<f32> = noisy.iter().map(|v| v * 0.5).collect();
	assert!((si_sdr_db(&clean, &scaled_noisy).unwrap() - sdr).abs() < 0.01);
}

#[test]
fn snr_mix_hits_the_target() {
	let clean = speech_like(2.0, 48_000, 1);
	let n = noise(NoiseKind::Brown, 2.0, 48_000, 5);
	for snr in [0.0, 10.0] {
		let mixed = mix_at_snr(&clean, &n, snr);
		let resid: Vec<f32> = mixed.iter().zip(&clean).map(|(m, c)| m - c).collect();
		let measured = 20.0 * (rms(&clean) / rms(&resid)).log10();
		assert!((measured - snr).abs() < 0.2, "{measured} vs {snr}");
	}
	assert!(mix_at_snr(&[0.0; 10], &[0.1; 10], 5.0).is_empty());
}

#[test]
fn correlation_finds_a_known_delay() {
	let x = speech_like(2.0, 48_000, 1);
	let mut y = vec![0.0; 137];
	y.extend_from_slice(&x);
	assert_eq!(correlation_delay(&x, &y, 960), Some(137));
	let (r, o) = align(&x, &y, 137, 100);
	assert_eq!(r, o);
}

#[test]
fn wav_round_trips_within_quantisation() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("a.wav");
	let x = speech_like(1.0, 48_000, 1);
	write_mono_48k(&path, &x).unwrap();
	let back = read_mono_48k(&path).unwrap();
	assert_eq!(back.len(), x.len());
	assert!(x.iter().zip(&back).all(|(a, b)| (a - b).abs() < 1e-4));
}

#[test]
fn corpus_round_trips_and_runner_records_off_baseline() {
	let dir = tempfile::tempdir().unwrap();
	let generated = generate_synthetic(dir.path(), 2.0).unwrap();
	let found = discover(dir.path()).unwrap();
	assert_eq!(found.len(), generated.len());
	let report = run(&["off".to_owned()], &found).unwrap();
	assert_eq!(report.status, "measured");
	let backend = &report.backends[0];
	assert_eq!(backend.delay_impulse_samples, Some(0));
	for file in &backend.files {
		assert_eq!(file.delay_xcorr_samples, Some(0));
		assert_eq!(file.samples_consumed, file.samples_produced);
		assert!(file.si_sdr_improvement_db.unwrap().abs() < 1e-6);
		assert!(file.timing.as_ref().unwrap().p50_ns <= file.timing.as_ref().unwrap().max_ns);
	}
	assert!(run(&["nope".to_owned()], &found).is_err());
}

#[test]
fn filtered_sdr_tolerates_frequency_dependent_delay() {
	use relay_benches::audio::metrics::filtered_sdr_db;
	let x = speech_like(4.0, 48_000, 1);
	// A multi-tap response: main path plus an earlier and a later echo.
	let y: Vec<f32> = (0..x.len())
		.map(|i| {
			let at = |d: i64| {
				let j = i as i64 - d;
				x.get(usize::try_from(j).unwrap_or(usize::MAX))
					.copied()
					.unwrap_or(0.0)
			};
			0.6 * at(0) + 0.3 * at(40) - 0.2 * at(-25)
		})
		.collect();
	let plain = si_sdr_db(&x, &y).unwrap();
	let filtered = filtered_sdr_db(&x, &y).unwrap();
	assert!(
		plain < 12.0,
		"plain SI-SDR is hurt by the extra taps: {plain}"
	);
	assert!(filtered > 60.0, "a linear filter explains it: {filtered}");
	// Independent noise is not explained by any filter.
	let n = noise(NoiseKind::White, 4.0, 48_000, 3);
	assert!(filtered_sdr_db(&x, &n).unwrap() < 5.0);
}
