//! Deterministic real-speech corpus builder (LibriSpeech utterances mixed
//! with DEMAND noise). Source data is read-only and never enters git.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::synth::{Rng, mix_at_snr};
use super::wav::{flac_duration_s, read_mono_48k, write_mono_48k};
use super::{BenchError, Result};

pub const SEED: u64 = 0x5245_4c41_5931;
pub const UTTERANCES: usize = 24;
pub const SPEAKERS: usize = 8;
pub const SNRS_DB: [i32; 5] = [-5, 0, 5, 10, 20];
const MIN_SECONDS: f64 = 5.0;
const MAX_SECONDS: f64 = 12.0;
const NOISES: [(&str, &str); 3] = [
	("kitchen", "demand/extracted/DKITCHEN_16k/DKITCHEN/ch01.wav"),
	("living", "demand/extracted/DLIVING_16k/DLIVING/ch01.wav"),
	("traffic", "demand/extracted/STRAFFIC_16k/STRAFFIC/ch01.wav"),
];

#[derive(Debug, Serialize)]
pub struct UtteranceEntry {
	pub index: usize,
	pub speaker: String,
	pub path: String,
	pub duration_s: f64,
	pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct NoiseEntry {
	pub name: &'static str,
	pub path: String,
	pub sha256: String,
	pub duration_s: f64,
}

#[derive(Debug, Serialize)]
pub struct ConditionEntry {
	pub name: String,
	pub utterance: usize,
	pub noise: Option<&'static str>,
	pub noise_offset_samples: Option<usize>,
	pub snr_db: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
	pub seed: u64,
	pub selection_rule: &'static str,
	pub mixing_rule: &'static str,
	pub archive_sha256: Vec<(String, String)>,
	pub utterances: Vec<UtteranceEntry>,
	pub noises: Vec<NoiseEntry>,
	pub conditions: Vec<ConditionEntry>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
	hex::encode(Sha256::digest(bytes))
}

fn sorted_children(dir: &Path) -> Result<Vec<PathBuf>> {
	let mut v: Vec<PathBuf> = fs::read_dir(dir)?
		.map(|e| e.map(|e| e.path()))
		.collect::<std::io::Result<_>>()?;
	v.sort();
	Ok(v)
}

fn flacs_of(speaker_dir: &Path) -> Result<Vec<PathBuf>> {
	let mut out = Vec::new();
	for chapter in sorted_children(speaker_dir)? {
		for file in sorted_children(&chapter)? {
			if file.extension().and_then(|e| e.to_str()) == Some("flac") {
				out.push(file);
			}
		}
	}
	Ok(out)
}

/// Pick the fixed subset: every `n/8`-th speaker, three utterances each
/// (5-12 s long, evenly spread over the speaker's sorted list).
fn select_utterances(root: &Path) -> Result<Vec<PathBuf>> {
	let speakers = sorted_children(&root.join("librispeech/extracted/LibriSpeech/dev-clean"))?;
	if speakers.len() < SPEAKERS {
		return Err(BenchError::Corpus("too few speakers".into()));
	}
	let per_speaker = UTTERANCES / SPEAKERS;
	let mut chosen = Vec::new();
	for s in 0..SPEAKERS {
		let speaker = &speakers[s * speakers.len() / SPEAKERS];
		let mut usable = Vec::new();
		for file in flacs_of(speaker)? {
			let d = flac_duration_s(&file)?;
			if (MIN_SECONDS..=MAX_SECONDS).contains(&d) {
				usable.push(file);
			}
		}
		if usable.len() < per_speaker {
			return Err(BenchError::Corpus(format!(
				"speaker {} has too few usable files",
				speaker.display()
			)));
		}
		for j in 0..per_speaker {
			chosen.push(
				usable[j * usable.len() / per_speaker + usable.len() / (2 * per_speaker)].clone(),
			);
		}
	}
	Ok(chosen)
}

fn snr_tag(snr: i32) -> String {
	if snr < 0 {
		format!("snrm{:02}", -snr)
	} else {
		format!("snr{snr:02}")
	}
}

/// Build `<out>/clean`, `<out>/noisy` and `<out>/manifest.json`. Returns the
/// manifest SHA-256.
pub fn build(corpus_root: &Path, out: &Path) -> Result<String> {
	fs::create_dir_all(out.join("clean"))?;
	fs::create_dir_all(out.join("noisy"))?;
	let rel = |p: &Path| {
		p.strip_prefix(corpus_root)
			.unwrap_or(p)
			.display()
			.to_string()
	};

	let mut noises = Vec::new();
	let mut noise_audio = Vec::new();
	for (name, path) in NOISES {
		let full = corpus_root.join(path);
		let samples = read_mono_48k(&full)?;
		noises.push(NoiseEntry {
			name,
			path: path.to_owned(),
			sha256: sha256_hex(&fs::read(&full)?),
			duration_s: samples.len() as f64 / 48_000.0,
		});
		noise_audio.push(samples);
	}

	let picked = select_utterances(corpus_root)?;
	let mut utterances = Vec::new();
	let mut conditions = Vec::new();
	for (u, file) in picked.iter().enumerate() {
		let speaker = file
			.ancestors()
			.nth(2)
			.and_then(|p| p.file_name())
			.map(|s| s.to_string_lossy().into_owned())
			.unwrap_or_default();
		utterances.push(UtteranceEntry {
			index: u,
			speaker: speaker.clone(),
			path: rel(file),
			duration_s: flac_duration_s(file)?,
			sha256: sha256_hex(&fs::read(file)?),
		});
		let clean = read_mono_48k(file)?;
		let id = format!(
			"u{u:02}-{}",
			file.file_stem()
				.map(|s| s.to_string_lossy().into_owned())
				.unwrap_or_default()
		);

		let clean_only = format!("{id}_clean");
		write_mono_48k(&out.join("clean").join(format!("{clean_only}.wav")), &clean)?;
		write_mono_48k(&out.join("noisy").join(format!("{clean_only}.wav")), &clean)?;
		conditions.push(ConditionEntry {
			name: clean_only,
			utterance: u,
			noise: None,
			noise_offset_samples: None,
			snr_db: None,
		});

		for (s, &snr) in SNRS_DB.iter().enumerate() {
			let n = (u + s) % NOISES.len();
			let noise = &noise_audio[n];
			let mut rng = Rng::new(SEED ^ ((u as u64) << 16) ^ s as u64);
			let offset = (rng.next_u64() % (noise.len() - clean.len()) as u64) as usize;
			let mixed = mix_at_snr(&clean, &noise[offset..offset + clean.len()], f64::from(snr));
			if mixed.is_empty() {
				return Err(BenchError::Corpus("silent speech or noise segment".into()));
			}
			let name = format!("{id}_{}_{}", snr_tag(snr), NOISES[n].0);
			write_mono_48k(&out.join("clean").join(format!("{name}.wav")), &clean)?;
			write_mono_48k(&out.join("noisy").join(format!("{name}.wav")), &mixed)?;
			conditions.push(ConditionEntry {
				name,
				utterance: u,
				noise: Some(NOISES[n].0),
				noise_offset_samples: Some(offset),
				snr_db: Some(snr),
			});
		}
	}

	let archive_sha256 = fs::read_to_string(corpus_root.join("SHA256SUMS"))?
		.lines()
		.filter_map(|l| l.split_once("  "))
		.map(|(h, p)| (p.to_owned(), h.to_owned()))
		.collect();
	let manifest = Manifest {
		seed: SEED,
		selection_rule: "8 speakers at evenly spaced sorted indices; 3 utterances each of 5-12 s at evenly spaced sorted positions",
		mixing_rule: "48 kHz resampled; noise = ch01 of a DEMAND scene chosen (u+snr_index) mod 3, seeded offset; noise scaled by whole-utterance RMS to the SNR; samples saturated at full scale; plus a clean-only copy per utterance",
		archive_sha256,
		utterances,
		noises,
		conditions,
	};
	let json = serde_json::to_string_pretty(&manifest)?;
	fs::write(out.join("manifest.json"), &json)?;
	Ok(sha256_hex(json.as_bytes()))
}
