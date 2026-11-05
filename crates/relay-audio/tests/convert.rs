use relay_audio::convert::{ChannelSelect, Sample, StreamResampler, fan_out};

#[test]
fn integer_formats_round_trip_and_saturate() {
	assert_eq!(i16::from_f32(1.5), i16::MAX);
	assert_eq!(i16::from_f32(-1.5), i16::MIN);
	assert_eq!(i16::from_f32(f32::NAN), 0);
	assert_eq!(u16::from_f32(f32::INFINITY), 0x8000);
	assert!((0.25_f32.to_f32() - 0.25).abs() < 1e-9);
	let s = i16::from_f32(0.5);
	assert!((s.to_f32() - 0.5).abs() < 1e-4);
	assert_eq!(i32::from_f32(2.0), i32::MAX);
}

#[test]
fn channel_selection_is_explicit() {
	let stereo = [0.5_f32, -0.5, 0.25, -0.25];
	let mut out = [0.0; 2];
	ChannelSelect::Channel(1).to_mono(&stereo, 2, &mut out);
	assert_eq!(out, [-0.5, -0.25]);
	ChannelSelect::Average.to_mono(&stereo, 2, &mut out);
	assert_eq!(out, [0.0, 0.0]);
	assert!(!ChannelSelect::Channel(2).is_valid_for(2));
	assert!(ChannelSelect::Average.is_valid_for(2));
}

#[test]
fn fan_out_duplicates_mono_and_saturates() {
	let mut out = [0_i16; 4];
	fan_out(&[0.5, 2.0], 2, &mut out);
	assert_eq!(out[0], out[1]);
	assert_eq!(out[2], i16::MAX);
	assert_eq!(out[3], i16::MAX);
}

#[test]
fn equal_rates_bypass_exactly() {
	let mut r = StreamResampler::new_fixed(48_000, 48_000, 480).unwrap();
	let input: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
	let mut got = Vec::new();
	r.push(&input, |b| got.extend_from_slice(b)).unwrap();
	assert_eq!(got, input);
	assert_eq!((r.consumed(), r.produced()), (1000, 1000));
	assert_eq!(r.delay_samples(), 0);
}

fn tone(rate: u32, n: usize) -> Vec<f32> {
	(0..n)
		.map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / rate as f32).sin() * 0.5)
		.collect()
}

#[test]
fn resampling_keeps_counts_and_signal_level() {
	for (from, to) in [
		(44_100_u32, 48_000_u32),
		(48_000, 16_000),
		(16_000, 48_000),
		(96_000, 48_000),
	] {
		let mut r = StreamResampler::new(from, to, (from / 100) as usize).unwrap();
		let input = tone(from, from as usize); // one second
		let mut got = Vec::new();
		// Feed in awkward slices that do not align with the chunk size.
		for piece in input.chunks(333) {
			r.push(piece, |b| got.extend_from_slice(b)).unwrap();
		}
		assert_eq!(r.produced() as usize, got.len());
		let expected = to as usize;
		let tolerance = expected / 50 + 64;
		assert!(
			got.len().abs_diff(expected) < tolerance,
			"{from}->{to}: got {} expected ~{expected}",
			got.len()
		);
		let tail = &got[got.len() / 2..];
		let rms = (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt();
		assert!(
			(rms - 0.5 / 2.0_f32.sqrt()).abs() < 0.03,
			"{from}->{to} rms {rms}"
		);
	}
}

#[test]
fn drift_adjustment_is_clamped() {
	let mut r = StreamResampler::new(44_100, 48_000, 441).unwrap();
	// A wild request must not error: it is clamped to the safe range.
	r.set_drift_ppm(1.0e6).unwrap();
	r.set_drift_ppm(-1.0e6).unwrap();
	let mut count = 0;
	r.push(&tone(44_100, 44_100), |b| count += b.len()).unwrap();
	assert!(count.abs_diff(48_000) < 1500);
}

#[test]
fn invalid_arguments_are_rejected() {
	assert!(StreamResampler::new(0, 48_000, 480).is_err());
	assert!(StreamResampler::new(48_000, 44_100, 0).is_err());
}

#[test]
fn drift_correction_works_at_48k() {
	let input = tone(48_000, 48_000 * 20);
	let mut baseline = 0;
	for ppm in [0.0, 100.0, -100.0, 300.0, -300.0] {
		let mut r = StreamResampler::new(48_000, 48_000, 480).unwrap();
		r.set_drift_ppm(ppm).unwrap();
		let mut count = 0_usize;
		for piece in input.chunks(480) {
			r.push(piece, |b| count += b.len()).unwrap();
		}
		if ppm == 0.0 {
			baseline = count;
			continue;
		}
		let extra = count as f64 - baseline as f64;
		let expected = input.len() as f64 * ppm * 1e-6;
		assert!(
			(extra - expected).abs() < 8.0,
			"{ppm} ppm: {extra} extra samples, expected {expected}"
		);
	}
}

#[test]
fn drift_beyond_the_clamp_is_limited_and_fixed_mode_refuses() {
	let input = tone(48_000, 48_000 * 10);
	let mut r = StreamResampler::new(48_000, 48_000, 480).unwrap();
	r.set_drift_ppm(5_000.0).unwrap();
	let mut count = 0_usize;
	r.push(&input, |b| count += b.len()).unwrap();
	let max_extra = input.len() as f64 * 300e-6;
	assert!((count as f64 - input.len() as f64) < max_extra + 70.0);
	let mut fixed = StreamResampler::new_fixed(48_000, 48_000, 480).unwrap();
	assert!(fixed.set_drift_ppm(100.0).is_err());
}
