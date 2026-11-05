//! The playback FIFO drift loop, simulated with a device clock that is
//! slightly off. Simulation only: no hardware clock was measured.

use relay_audio::playback::playback_pair;
use relay_engine::media::RenderSink;

// A 44.1 kHz device: at 48 kHz the resampler is bypassed and ignores
// drift correction (see docs/engine.md, known gaps).
const RATE: u32 = 44_100;

/// Renders 3 simulated minutes; the device consumes `480 * (1 + ppm / 1e6)`
/// samples per 10 ms of mixer time. Returns the final correction and the
/// FIFO fill range (sampled right after the device drained its share)
/// over the last 30 simulated seconds. The loop steers the mean fill, about
/// half a frame above this minimum, toward the 20 ms target (882 samples).
fn simulate(device_ppm: f64) -> (f64, usize, usize, u64) {
	let (mut callback, writer, shared) = playback_pair(1, 1_764);
	let mut sink = RenderSink::new(writer, RATE, 20).unwrap();
	let frame = [0.1f32; 480];
	let mut owed = 0.0f64;
	let mut out = vec![0.0f32; 480];
	let (mut low, mut high) = (usize::MAX, 0);
	// The render worker leads the device by the target fill.
	for _ in 0..2 {
		sink.write(&frame).unwrap();
	}
	let ticks = 18_000;
	for tick in 0..ticks {
		sink.write(&frame).unwrap();
		owed += 441.0 * (1.0 + device_ppm * 1e-6);
		let take = owed as usize;
		owed -= take as f64;
		callback.process(&mut out[..take.min(480)]);
		if tick >= ticks - 3_000 {
			let fill = sink.occupancy();
			low = low.min(fill);
			high = high.max(fill);
		}
	}
	(sink.drift_ppm(), low, high, shared.overflow_frames())
}

#[test]
fn a_slow_device_clock_is_followed_without_overflow() {
	// Device consumes 150 ppm too little, so the FIFO would fill by 1 sample
	// every 6.7 ms of audio without correction.
	let (ppm, low, high, overflow) = simulate(-150.0);
	assert!((-300.0..-50.0).contains(&ppm), "applied correction {ppm}");
	assert!((450..=800).contains(&low) && high <= 900, "{low}..{high}");
	assert_eq!(overflow, 0);
}

#[test]
fn a_fast_device_clock_is_followed_without_underrun() {
	let (ppm, low, high, _) = simulate(150.0);
	assert!((50.0..=300.0).contains(&ppm), "applied correction {ppm}");
	assert!(low >= 450 && high <= 900, "{low}..{high}");
}

#[test]
fn matched_clocks_stay_near_zero_correction() {
	let (ppm, low, high, overflow) = simulate(0.0);
	assert!(ppm.abs() < 30.0, "{ppm}");
	assert!((450..=800).contains(&low) && high <= 900, "{low}..{high}");
	assert_eq!(overflow, 0);
}
