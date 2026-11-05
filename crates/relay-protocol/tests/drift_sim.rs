use relay_protocol::drift::{DriftConfig, DriftEstimator};

const FRAME: u32 = 480;
const TARGET: u32 = 960;

/// Deterministic xorshift noise in `[-amp, amp]`.
struct Noise(u64);

impl Noise {
	fn next(&mut self, amp: f64) -> f64 {
		self.0 ^= self.0 << 13;
		self.0 ^= self.0 >> 7;
		self.0 ^= self.0 << 17;
		((self.0 % 2001) as f64 / 1000.0 - 1.0) * amp
	}
}

struct Outcome {
	final_error_ppm: f64,
	max_ratio_ppm: f64,
	max_occupancy_error: f64,
	tail_occupancy_error: f64,
}

/// Simulates `seconds` of a call where the sender clock runs `drift_ppm`
/// faster than the receiver clock. Occupancy is the sample-granular fill of
/// the render FIFO observed with arrival-burst noise, plus an optional stall dip.
fn simulate(drift_ppm: f64, seconds: u32, stall: Option<(u32, u32)>) -> Outcome {
	let mut est = DriftEstimator::new(DriftConfig::DEFAULT, TARGET);
	let mut noise = Noise(0x9e37_79b9_7f4a_7c15);
	let mut occupancy = f64::from(TARGET);
	let mut ratio_ppm = 0.0;
	let ticks = seconds * 100;
	let (mut max_ratio, mut max_err, mut tail_err) = (0.0f64, 0.0f64, 0.0f64);

	for tick in 0..ticks {
		occupancy += f64::from(FRAME) * (drift_ppm - ratio_ppm) * 1e-6;
		let mut observed = occupancy + noise.next(240.0);
		if let Some((start, len)) = stall
			&& (start..start + len).contains(&tick)
		{
			observed -= 2400.0;
		}
		let observed = observed.max(0.0) as u32;
		ratio_ppm = est.update(observed, FRAME);
		max_ratio = max_ratio.max(ratio_ppm.abs());
		let err = (occupancy - f64::from(TARGET)).abs();
		if tick > ticks / 3 {
			max_err = max_err.max(err);
		}
		if tick > ticks * 9 / 10 {
			tail_err = tail_err.max(err);
		}
	}
	Outcome {
		final_error_ppm: (ratio_ppm - drift_ppm).abs(),
		max_ratio_ppm: max_ratio,
		max_occupancy_error: max_err,
		tail_occupancy_error: tail_err,
	}
}

#[test]
fn converges_for_each_simulated_drift() {
	for drift in [0.0, 100.0, -100.0, 300.0, -300.0, 50.0] {
		let out = simulate(drift, 900, None);
		assert!(
			out.final_error_ppm < 15.0,
			"{drift} ppm: error {}",
			out.final_error_ppm
		);
		assert!(out.max_ratio_ppm <= 300.0 + 1e-9);
		assert!(
			out.tail_occupancy_error < 480.0,
			"{drift} ppm: occupancy {}",
			out.tail_occupancy_error
		);
		assert!(
			out.max_occupancy_error < 1200.0,
			"{drift} ppm: {}",
			out.max_occupancy_error
		);
	}
}

#[test]
fn saturates_instead_of_chasing_excess_drift() {
	let out = simulate(500.0, 300, None);
	assert!(out.max_ratio_ppm <= 300.0 + 1e-9);
}

#[test]
fn network_stall_does_not_retune_the_clock() {
	// 50 ms stall (5 ticks of missing audio) after the loop settled.
	let out = simulate(100.0, 900, Some((30_000, 5)));
	assert!(out.final_error_ppm < 15.0, "error {}", out.final_error_ppm);
	// A longer 500 ms outage must not leave the ratio far from truth either.
	let out = simulate(-100.0, 900, Some((30_000, 50)));
	assert!(out.final_error_ppm < 15.0, "error {}", out.final_error_ppm);
}

#[test]
fn ratio_changes_slowly() {
	let mut est = DriftEstimator::new(DriftConfig::DEFAULT, TARGET);
	est.update(TARGET, FRAME);
	let mut previous = est.ratio();
	for _ in 0..1000 {
		let ratio = est.update(TARGET + 400, FRAME);
		let r = 1.0 + ratio * 1e-6;
		// slew limit 5 ppm/s => 0.05 ppm per 10 ms tick
		assert!((r - previous).abs() <= 0.05e-6 + 1e-12);
		previous = r;
	}
	assert!(est.snapshot().ratio_ppm <= 300.0);
}
