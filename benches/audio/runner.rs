//! Runs suppressor candidates over a corpus and records only what was observed.

use std::time::Instant;

use cpu_time::ProcessTime;
use relay_audio::dsp::{
	DspError, DspLimits, NoiseSuppressor, OffSuppressor, measure_delay_samples,
};
use relay_audio::format::{BlockTiming, FRAME_SAMPLES};
use serde::Serialize;

use super::corpus::Pair;
use super::metrics::{align, correlation_delay, si_sdr_db};
use super::stoi::stoi;
use super::summary::{ConditionSummary, DelaySummary, by_condition, delays};
use super::wav::read_mono_48k;
use super::{BenchError, Result};

/// Samples at the start excluded from quality metrics (model warm-up).
const SKIP_SAMPLES: usize = 24_000;
const MAX_LAG: usize = 960;

#[derive(Debug, Clone, Serialize)]
pub struct FrameTiming {
	pub frames: usize,
	pub min_ns: u64,
	pub p50_ns: u64,
	pub p99_ns: u64,
	pub max_ns: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileResult {
	pub name: String,
	pub duration_s: f64,
	pub timing: Option<FrameTiming>,
	/// Process CPU time across the whole file loop (includes render feeding).
	pub cpu_ns: u64,
	/// CPU seconds per audio second; a single core fully busy would be 1.0.
	pub cpu_core_equivalent: f64,
	pub delay_xcorr_samples: Option<i64>,
	pub si_sdr_noisy_db: Option<f64>,
	pub si_sdr_output_db: Option<f64>,
	pub si_sdr_improvement_db: Option<f64>,
	pub stoi_noisy: Option<f64>,
	pub stoi_output: Option<f64>,
	pub estoi_noisy: Option<f64>,
	pub estoi_output: Option<f64>,
	pub samples_consumed: u64,
	pub samples_produced: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackendResult {
	pub backend: String,
	pub version: String,
	pub provides: Vec<String>,
	pub prepare_ns: u64,
	pub delay_impulse_samples: Option<usize>,
	pub callback_allocations: Option<u64>,
	/// Per-frame timing over every frame of every file.
	pub overall_timing: Option<FrameTiming>,
	/// Total CPU seconds per audio second over the whole corpus.
	pub cpu_core_equivalent: f64,
	pub delay_xcorr: Option<DelaySummary>,
	pub by_condition: Vec<ConditionSummary>,
	pub files: Vec<FileResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
	pub schema_version: u32,
	pub status: &'static str,
	pub os: &'static str,
	pub arch: &'static str,
	pub build_profile: &'static str,
	pub corpus_files: usize,
	pub backends: Vec<BackendResult>,
	pub notes: Vec<&'static str>,
}

/// A candidate plus whether it needs a (silent) render reference each frame.
pub(crate) struct Candidate {
	pub ns: Box<dyn NoiseSuppressor>,
	pub feed_render: bool,
}

pub(crate) fn make_candidate(name: &str) -> Result<Candidate> {
	let plain = |ns: Box<dyn NoiseSuppressor>| Candidate {
		ns,
		feed_render: false,
	};
	match name {
		"off" => Ok(plain(Box::<OffSuppressor>::default())),
		#[cfg(feature = "sonora")]
		"sonora-ns" | "sonora-ns-agc" | "sonora-ns-aec" => {
			use relay_audio::dsp::eval::{SonoraProfile, SonoraSuppressor};
			let profile = match name {
				"sonora-ns" => SonoraProfile::NsOnly,
				"sonora-ns-agc" => SonoraProfile::NsAgc,
				_ => SonoraProfile::NsAec,
			};
			Ok(Candidate {
				ns: Box::new(SonoraSuppressor::new(profile)),
				feed_render: profile == SonoraProfile::NsAec,
			})
		}
		#[cfg(feature = "nnnoiseless")]
		"nnnoiseless" => Ok(plain(Box::new(
			relay_audio::dsp::eval::NnnoiselessSuppressor::new(),
		))),
		other => Err(BenchError::BackendUnavailable(other.to_owned())),
	}
}

fn dsp(e: DspError) -> BenchError {
	BenchError::Dsp(e.to_string())
}

fn percentile(sorted: &[u64], q: f64) -> u64 {
	sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

fn timing(mut ns: Vec<u64>) -> Option<FrameTiming> {
	if ns.is_empty() {
		return None;
	}
	ns.sort_unstable();
	Some(FrameTiming {
		frames: ns.len(),
		min_ns: ns[0],
		p50_ns: percentile(&ns, 0.5),
		p99_ns: percentile(&ns, 0.99),
		max_ns: ns[ns.len() - 1],
	})
}

fn run_file(
	candidate: &mut Candidate,
	pair: &Pair,
	all_times: &mut Vec<u64>,
) -> Result<FileResult> {
	let noisy = read_mono_48k(&pair.noisy)?;
	let clean = read_mono_48k(&pair.clean)?;
	let frames = noisy.len() / FRAME_SAMPLES;
	let mut output = vec![0.0_f32; frames * FRAME_SAMPLES];
	let mut scratch = vec![0.0_f32; FRAME_SAMPLES * 2];
	let silence = vec![0.0_f32; FRAME_SAMPLES];
	let mut times = Vec::with_capacity(frames);
	let (mut consumed, mut produced) = (0_u64, 0_u64);

	let cpu_start = ProcessTime::now();
	for (n, frame) in noisy.as_chunks::<FRAME_SAMPLES>().0.iter().enumerate() {
		if candidate.feed_render {
			candidate.ns.push_render(&silence).map_err(dsp)?;
		}
		let timing = BlockTiming {
			generation: 0,
			first_sample: (n * FRAME_SAMPLES) as u64,
		};
		let started = Instant::now();
		let progress = candidate
			.ns
			.process_into(frame, &mut scratch, timing)
			.map_err(dsp)?;
		times.push(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
		consumed += progress.consumed as u64;
		produced += progress.produced as u64;
		output[n * FRAME_SAMPLES..(n + 1) * FRAME_SAMPLES]
			.copy_from_slice(&scratch[..FRAME_SAMPLES]);
	}
	let cpu = cpu_start.elapsed();

	let duration_s = (frames * FRAME_SAMPLES) as f64 / 48_000.0;
	let delay = correlation_delay(&noisy, &output, MAX_LAG);
	let (ref_out, out) = align(&clean, &output, delay.unwrap_or(0), SKIP_SAMPLES);
	let si_out = si_sdr_db(ref_out, out);
	let (ref_in, inp) = align(&clean, &noisy, 0, SKIP_SAMPLES);
	let si_in = si_sdr_db(ref_in, inp);
	let score_out = stoi(ref_out, out, 48_000);
	let score_in = stoi(ref_in, inp, 48_000);
	all_times.extend_from_slice(&times);
	Ok(FileResult {
		name: pair.name.clone(),
		duration_s,
		timing: timing(times),
		cpu_ns: u64::try_from(cpu.as_nanos()).unwrap_or(u64::MAX),
		cpu_core_equivalent: cpu.as_secs_f64() / duration_s.max(f64::EPSILON),
		delay_xcorr_samples: delay,
		si_sdr_noisy_db: si_in,
		si_sdr_output_db: si_out,
		si_sdr_improvement_db: si_out.zip(si_in).map(|(o, i)| o - i),
		stoi_noisy: score_in.map(|s| s.stoi),
		stoi_output: score_out.map(|s| s.stoi),
		estoi_noisy: score_in.map(|s| s.estoi),
		estoi_output: score_out.map(|s| s.estoi),
		samples_consumed: consumed,
		samples_produced: produced,
	})
}

pub fn run_backend(name: &str, pairs: &[Pair]) -> Result<BackendResult> {
	let mut probe = make_candidate(name)?;
	let limits = DspLimits {
		max_block_samples: FRAME_SAMPLES,
		max_scratch_bytes: probe.ns.descriptor().max_scratch_bytes,
	};
	let started = Instant::now();
	probe.ns.prepare(&limits).map_err(dsp)?;
	let prepare_ns = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
	let delay_impulse =
		measure_delay_samples(probe.ns.as_mut(), FRAME_SAMPLES, 9_600).map_err(dsp)?;
	let descriptor = probe.ns.descriptor().clone();

	let mut files = Vec::new();
	let mut all_times = Vec::new();
	for pair in pairs {
		// Fresh state per file so results do not depend on corpus order.
		let mut candidate = make_candidate(name)?;
		candidate.ns.prepare(&limits).map_err(dsp)?;
		files.push(run_file(&mut candidate, pair, &mut all_times)?);
	}
	Ok(BackendResult {
		backend: descriptor.backend_id.clone(),
		version: descriptor.version.clone(),
		provides: relay_audio::dsp::StageKind::ORDER
			.into_iter()
			.filter(|k| descriptor.provides.contains(*k))
			.map(|k| format!("{k:?}"))
			.collect(),
		prepare_ns,
		delay_impulse_samples: delay_impulse,
		callback_allocations: None,
		overall_timing: timing(all_times),
		cpu_core_equivalent: files.iter().map(|f| f.cpu_ns as f64 * 1e-9).sum::<f64>()
			/ files
				.iter()
				.map(|f| f.duration_s)
				.sum::<f64>()
				.max(f64::EPSILON),
		delay_xcorr: delays(&files),
		by_condition: by_condition(&files),
		files,
	})
}

pub fn run(names: &[String], pairs: &[Pair]) -> Result<Report> {
	let backends = names
		.iter()
		.map(|n| run_backend(n, pairs))
		.collect::<Result<Vec<_>>>()?;
	Ok(Report {
		schema_version: 1,
		status: "measured",
		os: std::env::consts::OS,
		arch: std::env::consts::ARCH,
		build_profile: if cfg!(debug_assertions) {
			"debug"
		} else {
			"release"
		},
		corpus_files: pairs.len(),
		backends,
		notes: vec![
			"STOI/eSTOI are this repo's implementation of the published algorithms; not validated against a reference implementation.",
			"callback_allocations stays null here; allocation behaviour is asserted in relay-audio tests.",
			"The AEC profile is fed silent render frames: only its processing cost is exercised.",
			"Files with no estimable quantity (e.g. identical signals for SI-SDR) are left out of means.",
			"Quality metrics skip the first 0.5 s and align output by cross-correlation delay.",
		],
	})
}

pub fn default_backends() -> Vec<String> {
	let mut names = vec!["off".to_owned()];
	if cfg!(feature = "sonora") {
		names.extend(["sonora-ns", "sonora-ns-agc", "sonora-ns-aec"].map(str::to_owned));
	}
	if cfg!(feature = "nnnoiseless") {
		names.push("nnnoiseless".to_owned());
	}
	names
}
