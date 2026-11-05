//! Trial selection and seeded shuffling.

use std::collections::BTreeMap;

use crate::audio::corpus::Pair;
use crate::audio::synth::Rng;

/// One utterance under one input condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trial {
	pub utterance: String,
	pub condition: String,
	pub pair_index: usize,
}

/// Split `u00-1-2-3_snr05_kitchen` into (`u00-1-2-3`, `snr05`).
pub fn split_name(name: &str) -> Option<(String, String)> {
	let tokens: Vec<&str> = name.split('_').collect();
	let at = tokens
		.iter()
		.position(|t| t.starts_with("snr") || *t == "clean")?;
	Some((tokens[..at].join("_"), tokens[at].to_owned()))
}

pub fn shuffle<T>(items: &mut [T], rng: &mut Rng) {
	for i in (1..items.len()).rev() {
		let j = (rng.next_u64() % (i as u64 + 1)) as usize;
		items.swap(i, j);
	}
}

/// Pick `utterances` distinct utterances (seeded) and one pair for each of
/// `conditions`. Utterances lacking a requested condition are skipped.
pub fn choose_trials(
	pairs: &[Pair],
	utterances: usize,
	conditions: &[String],
	rng: &mut Rng,
) -> Vec<Trial> {
	let mut by_utterance: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
	for (index, pair) in pairs.iter().enumerate() {
		if let Some((utterance, condition)) = split_name(&pair.name) {
			by_utterance
				.entry(utterance)
				.or_default()
				.entry(condition)
				.or_insert(index);
		}
	}
	let mut names: Vec<String> = by_utterance
		.iter()
		.filter(|(_, c)| conditions.iter().all(|w| c.contains_key(w)))
		.map(|(u, _)| u.clone())
		.collect();
	shuffle(&mut names, rng);
	names.truncate(utterances);
	let mut trials: Vec<Trial> = names
		.iter()
		.flat_map(|u| {
			conditions.iter().map(|c| Trial {
				utterance: u.clone(),
				condition: c.clone(),
				pair_index: by_utterance[u][c],
			})
		})
		.collect();
	shuffle(&mut trials, rng);
	trials
}
