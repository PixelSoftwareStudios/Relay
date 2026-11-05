//! Corpus layout: `<dir>/clean/NAME.wav` paired with `<dir>/noisy/NAME.wav`.

use std::fs;
use std::path::{Path, PathBuf};

use super::synth::{NoiseKind, mix_at_snr, noise, speech_like};
use super::wav::write_mono_48k;
use super::{BenchError, Result};

pub struct Pair {
	pub name: String,
	pub clean: PathBuf,
	pub noisy: PathBuf,
}

/// SNRs of the generated corpus, in dB.
pub const SYNTH_SNRS_DB: [f64; 2] = [0.0, 10.0];

/// Write a small deterministic synthetic corpus and return its pairs.
pub fn generate_synthetic(dir: &Path, seconds: f32) -> Result<Vec<Pair>> {
	fs::create_dir_all(dir.join("clean"))?;
	fs::create_dir_all(dir.join("noisy"))?;
	let clean = speech_like(seconds, 48_000, 1);
	let mut pairs = Vec::new();
	for (k, kind) in NoiseKind::ALL.into_iter().enumerate() {
		let noise = noise(kind, seconds, 48_000, 100 + k as u64);
		for snr in SYNTH_SNRS_DB {
			let name = format!("{}_snr{:02}", kind.name(), snr as i64);
			let mixed = mix_at_snr(&clean, &noise, snr);
			let (clean_path, noisy_path) = (
				dir.join("clean").join(format!("{name}.wav")),
				dir.join("noisy").join(format!("{name}.wav")),
			);
			write_mono_48k(&clean_path, &clean)?;
			write_mono_48k(&noisy_path, &mixed)?;
			pairs.push(Pair {
				name,
				clean: clean_path,
				noisy: noisy_path,
			});
		}
	}
	Ok(pairs)
}

/// Find every `noisy/*.wav` that has a same-named `clean/*.wav`.
pub fn discover(dir: &Path) -> Result<Vec<Pair>> {
	let mut pairs = Vec::new();
	for entry in fs::read_dir(dir.join("noisy"))? {
		let path = entry?.path();
		if path.extension().and_then(|e| e.to_str()) != Some("wav") {
			continue;
		}
		let file = path.file_name().map(PathBuf::from).unwrap_or_default();
		let clean = dir.join("clean").join(&file);
		if !clean.exists() {
			return Err(BenchError::Corpus(format!(
				"no clean reference for {}",
				file.display()
			)));
		}
		let name = path
			.file_stem()
			.map(|s| s.to_string_lossy().into_owned())
			.unwrap_or_default();
		pairs.push(Pair {
			name,
			clean,
			noisy: path,
		});
	}
	pairs.sort_by(|a, b| a.name.cmp(&b.name));
	if pairs.is_empty() {
		return Err(BenchError::Corpus("no wav pairs found".into()));
	}
	Ok(pairs)
}
