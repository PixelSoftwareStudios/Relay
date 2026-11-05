//! Clock-drift ratio estimation from playout-buffer occupancy.
//!
//! A slow PI loop: occupancy is low-passed, its error against the target
//! drives a proportional term and a clamped integral term. The integral
//! converges to the true relative clock drift while the output ratio is
//! saturated and slew-limited, so a transient network stall cannot move the
//! sample rate far or fast. Network margin and rate correction stay separate
//! loops; this one never reacts to a single late packet.

/// The integral may exceed the output limit slightly so that a drift sitting
/// exactly at the limit stays controllable instead of balancing on the clamp.
const INTEGRAL_HEADROOM: f64 = 1.25;

/// Estimator tuning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftConfig {
	/// Output saturation, parts per million.
	pub limit_ppm: f64,
	/// Largest ratio change per second, ppm.
	pub slew_ppm_per_s: f64,
	/// Occupancy low-pass time constant, seconds.
	pub filter_tau_s: f64,
	/// Proportional gain, ppm per sample of error.
	pub kp: f64,
	/// Integral gain, ppm per second per sample of error.
	pub ki: f64,
	/// Error magnitude (samples) the loop acts on; larger errors are clamped.
	pub error_clamp: f64,
	/// Raw deviation (samples) treated as a transient and kept out of the filter.
	pub transient_samples: f64,
	/// Consecutive transient seconds after which the new level is accepted.
	pub transient_accept_s: f64,
	pub sample_rate: f64,
}

impl DriftConfig {
	/// Defaults sized for the +-300 ppm initial drift limit.
	pub const DEFAULT: Self = Self {
		limit_ppm: 300.0,
		slew_ppm_per_s: 5.0,
		filter_tau_s: 2.0,
		kp: 0.5,
		ki: 0.02,
		error_clamp: 192.0,
		transient_samples: 960.0,
		transient_accept_s: 5.0,
		sample_rate: 48_000.0,
	};
}

/// Point-in-time view for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftSnapshot {
	/// Resampler ratio correction, ppm (positive = consume input faster).
	pub ratio_ppm: f64,
	pub filtered_occupancy: f64,
	pub target_occupancy: f64,
	/// 0.0 (unsettled) to 1.0 (error small and steady).
	pub confidence: f64,
	pub discarded_transients: u64,
}

/// Drift controller.
#[derive(Debug, Clone)]
pub struct DriftEstimator {
	cfg: DriftConfig,
	target: f64,
	filtered: Option<f64>,
	integral_ppm: f64,
	ratio_ppm: f64,
	transient_s: f64,
	settled_s: f64,
	discarded: u64,
}

impl DriftEstimator {
	pub fn new(cfg: DriftConfig, target_occupancy_samples: u32) -> Self {
		Self {
			cfg,
			target: f64::from(target_occupancy_samples),
			filtered: None,
			integral_ppm: 0.0,
			ratio_ppm: 0.0,
			transient_s: 0.0,
			settled_s: 0.0,
			discarded: 0,
		}
	}

	/// Changes the occupancy the loop steers towards (margin change).
	pub fn set_target(&mut self, samples: u32) {
		self.target = f64::from(samples);
	}

	/// Clears learned state after a discontinuity or new generation.
	pub fn reset(&mut self) {
		let target = self.target;
		*self = Self::new(self.cfg, 0);
		self.target = target;
	}

	/// Feeds one observation. `elapsed_samples` is the output time since the
	/// previous observation. Returns the ratio correction in ppm.
	pub fn update(&mut self, occupancy_samples: u32, elapsed_samples: u32) -> f64 {
		let dt = f64::from(elapsed_samples) / self.cfg.sample_rate;
		let occupancy = f64::from(occupancy_samples);
		let Some(filtered) = self.filtered else {
			self.filtered = Some(occupancy);
			return self.ratio_ppm;
		};

		if (occupancy - filtered).abs() > self.cfg.transient_samples {
			self.transient_s += dt;
			self.discarded += 1;
			if self.transient_s >= self.cfg.transient_accept_s {
				self.filtered = Some(occupancy);
				self.transient_s = 0.0;
			}
			return self.slew(self.desired(filtered), dt);
		}
		self.transient_s = 0.0;

		let alpha = (dt / self.cfg.filter_tau_s).min(1.0);
		let filtered = filtered + alpha * (occupancy - filtered);
		self.filtered = Some(filtered);

		let error = (filtered - self.target).clamp(-self.cfg.error_clamp, self.cfg.error_clamp);
		self.integral_ppm = (self.integral_ppm + self.cfg.ki * error * dt).clamp(
			-INTEGRAL_HEADROOM * self.cfg.limit_ppm,
			INTEGRAL_HEADROOM * self.cfg.limit_ppm,
		);
		if error.abs() < self.cfg.error_clamp / 2.0 {
			self.settled_s += dt;
		} else {
			self.settled_s = 0.0;
		}
		self.slew(self.desired(filtered), dt)
	}

	/// Ratio multiplier to apply to the resampler (`1 + ppm * 1e-6`).
	pub fn ratio(&self) -> f64 {
		1.0 + self.ratio_ppm * 1e-6
	}

	pub fn snapshot(&self) -> DriftSnapshot {
		DriftSnapshot {
			ratio_ppm: self.ratio_ppm,
			filtered_occupancy: self.filtered.unwrap_or(0.0),
			target_occupancy: self.target,
			confidence: (self.settled_s / 30.0).min(1.0),
			discarded_transients: self.discarded,
		}
	}

	fn desired(&self, filtered: f64) -> f64 {
		let error = (filtered - self.target).clamp(-self.cfg.error_clamp, self.cfg.error_clamp);
		(self.cfg.kp * error + self.integral_ppm).clamp(-self.cfg.limit_ppm, self.cfg.limit_ppm)
	}

	fn slew(&mut self, desired: f64, dt: f64) -> f64 {
		let step = self.cfg.slew_ppm_per_s * dt;
		self.ratio_ppm += (desired - self.ratio_ppm).clamp(-step, step);
		self.ratio_ppm
	}
}
