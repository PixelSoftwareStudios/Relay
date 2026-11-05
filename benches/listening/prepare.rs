//! Render, align, loudness-match and shuffle the clips of a session.

use std::path::{Path, PathBuf};

use relay_audio::dsp::DspLimits;
use relay_audio::format::{BlockTiming, FRAME_SAMPLES};

use super::key::{ANSWERS_FILE, CLIP_DIR, ClipKey, Key};
use super::select::{choose_trials, shuffle};
use super::{ListenError, Result, bad};
use crate::audio::corpus::discover;
use crate::audio::metrics::correlation_delay;
use crate::audio::runner::make_candidate;
use crate::audio::synth::Rng;
use crate::audio::wav::{read_mono_48k, write_mono_48k};

/// Widest delay compensated when lining clips up (100 ms at 48 kHz).
const MAX_LAG: usize = 4_800;
/// Loudness matching never amplifies or attenuates beyond this, in dB.
const MAX_GAIN_DB: f64 = 12.0;
const PEAK_LIMIT: f32 = 0.98;

#[derive(Debug, Clone)]
pub struct Options {
	pub corpus: PathBuf,
	pub out: PathBuf,
	pub backends: Vec<String>,
	pub utterances: usize,
	pub conditions: Vec<String>,
	pub seed: u64,
}

fn dsp(e: relay_audio::dsp::DspError) -> ListenError {
	ListenError::Bad(e.to_string())
}

/// Run a whole file through a fresh candidate (48 kHz mono in and out).
pub fn render(backend: &str, input: &[f32]) -> Result<Vec<f32>> {
	let mut candidate = make_candidate(backend)?;
	let limits = DspLimits {
		max_block_samples: FRAME_SAMPLES,
		max_scratch_bytes: candidate.ns.descriptor().max_scratch_bytes,
	};
	candidate.ns.prepare(&limits).map_err(dsp)?;
	let silence = vec![0.0_f32; FRAME_SAMPLES];
	let mut scratch = vec![0.0_f32; FRAME_SAMPLES * 2];
	let mut out = Vec::with_capacity(input.len());
	for (n, frame) in input.as_chunks::<FRAME_SAMPLES>().0.iter().enumerate() {
		if candidate.feed_render {
			candidate.ns.push_render(&silence).map_err(dsp)?;
		}
		let timing = BlockTiming {
			generation: 0,
			first_sample: (n * FRAME_SAMPLES) as u64,
		};
		let progress = candidate
			.ns
			.process_into(frame, &mut scratch, timing)
			.map_err(dsp)?;
		out.extend_from_slice(&scratch[..progress.produced]);
	}
	Ok(out)
}

pub fn rms(samples: &[f32]) -> f64 {
	if samples.is_empty() {
		return 0.0;
	}
	(samples.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
}

/// Drop `delay` leading samples and zero-pad so the length is unchanged.
pub fn remove_delay(output: &[f32], delay: i64) -> Vec<f32> {
	let d = usize::try_from(delay.max(0)).unwrap_or(0).min(output.len());
	let mut shifted = output[d..].to_vec();
	shifted.resize(output.len(), 0.0);
	shifted
}

/// Gain (linear) that brings `clip` to the loudness of `reference`, limited
/// to +/-12 dB and so the peak stays below full scale.
pub fn match_gain(reference: &[f32], clip: &[f32]) -> f64 {
	let (r, c) = (rms(reference), rms(clip));
	if r <= 0.0 || c <= 0.0 {
		return 1.0;
	}
	let limit = 10_f64.powf(MAX_GAIN_DB / 20.0);
	let peak = clip.iter().fold(0.0_f32, |p, s| p.max(s.abs()));
	let headroom = if peak > 0.0 {
		f64::from(PEAK_LIMIT / peak)
	} else {
		limit
	};
	(r / c).clamp(1.0 / limit, limit).min(headroom)
}

fn trial_letter(i: usize) -> char {
	char::from(b'A' + u8::try_from(i).unwrap_or(25).min(25))
}

pub fn prepare(options: &Options) -> Result<Key> {
	let distinct: std::collections::BTreeSet<&String> = options.backends.iter().collect();
	if options.backends.is_empty()
		|| options.backends.len() > 26
		|| distinct.len() != options.backends.len()
	{
		return bad("choose between 1 and 26 distinct backends");
	}
	if options.conditions.is_empty() || options.utterances == 0 {
		return bad("need at least one utterance and one condition");
	}
	let pairs = discover(&options.corpus)?;
	let mut rng = Rng::new(options.seed);
	let trials = choose_trials(&pairs, options.utterances, &options.conditions, &mut rng);
	if trials.is_empty() {
		return bad("no utterance has every requested condition");
	}
	let clip_dir = options.out.join(CLIP_DIR);
	std::fs::create_dir_all(&clip_dir)?;
	let mut clips = Vec::new();
	for (t, trial) in trials.iter().enumerate() {
		let noisy = read_mono_48k(&pairs[trial.pair_index].noisy)?;
		let mut order: Vec<&String> = options.backends.iter().collect();
		shuffle(&mut order, &mut rng);
		for (slot, backend) in order.into_iter().enumerate() {
			let rendered = render(backend, &noisy)?;
			let delay = correlation_delay(&noisy, &rendered, MAX_LAG).unwrap_or(0);
			let mut clip = remove_delay(&rendered, delay);
			let gain = match_gain(&noisy, &clip);
			clip.iter_mut()
				.for_each(|s| *s = (f64::from(*s) * gain) as f32);
			let name = format!("trial{:02}_{}", t + 1, trial_letter(slot));
			write_mono_48k(&clip_dir.join(format!("{name}.wav")), &clip)?;
			clips.push(ClipKey {
				clip: name,
				trial: t + 1,
				utterance: trial.utterance.clone(),
				condition: trial.condition.clone(),
				backend: backend.clone(),
				gain_db: 20.0 * gain.log10(),
				delay_removed_samples: delay,
			});
		}
	}
	let key = Key {
		schema_version: 1,
		seed: options.seed,
		backends: options.backends.clone(),
		clips,
	};
	key.save(&options.out)?;
	write_answer_sheet(&options.out, &key)?;
	Ok(key)
}

fn write_answer_sheet(session: &Path, key: &Key) -> Result<()> {
	let mut names: Vec<&str> = key.clips.iter().map(|c| c.clip.as_str()).collect();
	names.sort_unstable();
	let mut text = String::from("clip,rating,preferred\n");
	for name in names {
		text.push_str(&format!("{name},,\n"));
	}
	std::fs::write(session.join(ANSWERS_FILE), text)?;
	Ok(())
}
