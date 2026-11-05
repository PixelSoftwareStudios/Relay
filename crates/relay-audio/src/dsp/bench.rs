//! Measurement harness skeleton: records real timings only.
//!
//! Nothing here estimates, extrapolates or fills in values. Anything it
//! cannot observe stays `None` and serialises as `null`.

use std::time::Instant;

use serde::Serialize;

use super::{DspError, DspLimits, NoiseSuppressor};
use crate::format::BlockTiming;

#[derive(Debug, Clone, Copy)]
pub struct HarnessConfig {
	/// Samples offered per `process_into` call.
	pub block_samples: usize,
	pub warmup_blocks: usize,
	pub measured_blocks: usize,
}

impl Default for HarnessConfig {
	fn default() -> Self {
		Self {
			block_samples: crate::format::FRAME_SAMPLES,
			warmup_blocks: 100,
			measured_blocks: 1000,
		}
	}
}

/// Distribution of observed per-call execution times in nanoseconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BenchSummary {
	pub samples: usize,
	pub min_ns: u64,
	pub p50_ns: u64,
	pub p99_ns: u64,
	pub max_ns: u64,
}

impl BenchSummary {
	fn from_samples(mut samples: Vec<u64>) -> Option<Self> {
		if samples.is_empty() {
			return None;
		}
		samples.sort_unstable();
		let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
		Some(Self {
			samples: samples.len(),
			min_ns: samples[0],
			p50_ns: at(0.50),
			p99_ns: at(0.99),
			max_ns: samples[samples.len() - 1],
		})
	}
}

/// One run's results.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BenchReport {
	pub schema_version: u32,
	/// `measured` for a real run, `not_run` otherwise.
	pub status: &'static str,
	pub backend_id: String,
	pub os: Option<&'static str>,
	pub arch: Option<&'static str>,
	pub block_samples: usize,
	/// One-time `prepare` cost, reported apart from steady state.
	pub prepare_ns: Option<u64>,
	pub process: Option<BenchSummary>,
	pub samples_consumed: u64,
	pub samples_produced: u64,
	/// Measured with an impulse through the whole wrapper, if it was run.
	pub measured_delay_samples: Option<usize>,
	/// Supplied by a caller that actually counted allocations.
	pub callback_allocations: Option<u64>,
}

impl BenchReport {
	/// A placeholder carrying no results.
	pub fn not_run(backend_id: &str) -> Self {
		Self {
			schema_version: 1,
			status: "not_run",
			backend_id: backend_id.to_owned(),
			os: None,
			arch: None,
			block_samples: 0,
			prepare_ns: None,
			process: None,
			samples_consumed: 0,
			samples_produced: 0,
			measured_delay_samples: None,
			callback_allocations: None,
		}
	}

	pub fn to_json(&self) -> String {
		serde_json::to_string_pretty(self).unwrap_or_default()
	}
}

pub struct Harness {
	config: HarnessConfig,
}

impl Harness {
	pub fn new(config: HarnessConfig) -> Self {
		Self { config }
	}

	/// Time `process_into` over a deterministic source. Buffers are allocated
	/// before the loop; the sample-time vector is sized up front too.
	pub fn run(
		&self,
		suppressor: &mut dyn NoiseSuppressor,
		mut source: impl FnMut(u64) -> f32,
	) -> Result<BenchReport, DspError> {
		let n = self.config.block_samples;
		let limits = DspLimits {
			max_block_samples: n,
			max_scratch_bytes: suppressor.descriptor().max_scratch_bytes,
		};
		let started = Instant::now();
		suppressor.prepare(&limits)?;
		let prepare_ns = elapsed_ns(started);

		let mut input = vec![0.0_f32; n];
		let mut output = vec![0.0_f32; n * 2];
		let mut times = Vec::with_capacity(self.config.measured_blocks);
		let (mut index, mut consumed, mut produced) = (0_u64, 0_u64, 0_u64);

		for block in 0..self.config.warmup_blocks + self.config.measured_blocks {
			for slot in &mut input {
				*slot = source(index);
				index += 1;
			}
			let timing = BlockTiming {
				generation: 0,
				first_sample: index - n as u64,
			};
			let started = Instant::now();
			let progress = suppressor.process_into(&input, &mut output, timing)?;
			let took = elapsed_ns(started);
			consumed += progress.consumed as u64;
			produced += progress.produced as u64;
			if block >= self.config.warmup_blocks {
				times.push(took);
			}
		}
		Ok(BenchReport {
			schema_version: 1,
			status: "measured",
			backend_id: suppressor.descriptor().backend_id.clone(),
			os: Some(std::env::consts::OS),
			arch: Some(std::env::consts::ARCH),
			block_samples: n,
			prepare_ns: Some(prepare_ns),
			process: BenchSummary::from_samples(times),
			samples_consumed: consumed,
			samples_produced: produced,
			measured_delay_samples: None,
			callback_allocations: None,
		})
	}
}

fn elapsed_ns(started: Instant) -> u64 {
	u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Measure waveform-to-waveform delay of a prepared suppressor by passing a
/// unit impulse and locating it in the emitted stream. Positions count
/// emitted samples, so a backend cannot hide buffering by relabelling
/// indices. Returns `None` if the impulse never appears within `max_samples`.
pub fn measure_delay_samples(
	suppressor: &mut dyn NoiseSuppressor,
	block_samples: usize,
	max_samples: usize,
) -> Result<Option<usize>, DspError> {
	let impulse_at = block_samples * 4;
	let mut input = vec![0.0_f32; block_samples];
	let mut output = vec![0.0_f32; block_samples * 2];
	let (mut sent, mut emitted) = (0_usize, 0_usize);
	while sent < max_samples + impulse_at {
		for (i, slot) in input.iter_mut().enumerate() {
			*slot = if sent + i == impulse_at { 1.0 } else { 0.0 };
		}
		let timing = BlockTiming {
			generation: 0,
			first_sample: sent as u64,
		};
		sent += block_samples;
		let progress = suppressor.process_into(&input, &mut output, timing)?;
		let block = &output[..progress.produced];
		if let Some(pos) = block.iter().position(|v| v.abs() > 0.5) {
			return Ok(Some((emitted + pos).saturating_sub(impulse_at)));
		}
		emitted += progress.produced;
	}
	Ok(None)
}
