use relay_protocol::drift::{DriftConfig, DriftEstimator};
use relay_protocol::jitter::{JitterBuffer, JitterConfig, MarginController};
use relay_protocol::seq::{SeqExtender, TimestampExtender};

#[test]
fn extended_numbers_report_exhaustion_without_losing_the_reference() {
	let mut sequence = SeqExtender::new();
	sequence.commit(u64::MAX);
	assert!(sequence.try_extend(0).is_err());
	assert_eq!(sequence.highest(), Some(u64::MAX));
	assert_eq!(sequence.try_resolve(u16::MAX), Ok(u64::MAX));
	let mut timestamp = TimestampExtender::new();
	timestamp.commit(u64::MAX);
	assert!(timestamp.try_resolve(0).is_err());
	assert_eq!(timestamp.try_resolve(u32::MAX), Ok(u64::MAX));
}

#[test]
fn jitter_rejects_invalid_configuration_before_processing() {
	for config in [
		JitterConfig {
			slots: 0,
			..JitterConfig::BALANCED
		},
		JitterConfig {
			frame_samples: 0,
			..JitterConfig::BALANCED
		},
		JitterConfig {
			min_margin_frames: 5,
			max_margin_frames: 4,
			..JitterConfig::BALANCED
		},
		JitterConfig {
			max_margin_frames: u32::MAX,
			..JitterConfig::BALANCED
		},
		JitterConfig {
			max_plc_frames: u32::MAX,
			..JitterConfig::BALANCED
		},
	] {
		assert!(JitterBuffer::<()>::try_new(config).is_err(), "{config:?}");
		assert!(MarginController::try_new(&config).is_err(), "{config:?}");
		// Helpers remain total even on unvalidated input.
		assert!(config.horizon_frames() <= 127);
		let _ = config.max_delay_samples();
	}
	for config in [
		JitterConfig::BALANCED,
		JitterConfig::STABLE,
		JitterConfig::AGGRESSIVE,
	] {
		assert!(JitterBuffer::<()>::try_new(config).is_ok());
	}
}

#[test]
fn drift_rejects_nonfinite_negative_and_zero_divisors() {
	for value in [-1.0, f64::NAN, f64::INFINITY] {
		assert!(
			DriftEstimator::try_new(
				DriftConfig {
					limit_ppm: value,
					..DriftConfig::DEFAULT
				},
				480
			)
			.is_err()
		);
	}
	for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
		assert!(
			DriftEstimator::try_new(
				DriftConfig {
					sample_rate: value,
					..DriftConfig::DEFAULT
				},
				480
			)
			.is_err()
		);
		assert!(
			DriftEstimator::try_new(
				DriftConfig {
					filter_tau_s: value,
					..DriftConfig::DEFAULT
				},
				480
			)
			.is_err()
		);
	}
}

#[test]
fn drift_zero_elapsed_time_and_zero_gains_remain_finite() {
	let config = DriftConfig {
		limit_ppm: 0.0,
		slew_ppm_per_s: 0.0,
		kp: 0.0,
		ki: 0.0,
		..DriftConfig::DEFAULT
	};
	let mut estimator = DriftEstimator::try_new(config, 480).unwrap();
	for elapsed in [0, 480, u32::MAX] {
		assert!(estimator.update(u32::MAX, elapsed).is_finite());
	}
}

#[test]
fn finite_extreme_gains_do_not_introduce_nan_on_a_zero_length_tick() {
	let config = DriftConfig {
		kp: f64::MAX,
		ki: f64::MAX,
		slew_ppm_per_s: f64::MAX,
		..DriftConfig::DEFAULT
	};
	let mut estimator = DriftEstimator::try_new(config, 0).unwrap();
	for elapsed in [480, 0, 1, u32::MAX] {
		assert!(estimator.update(480, elapsed).is_finite());
	}
}
