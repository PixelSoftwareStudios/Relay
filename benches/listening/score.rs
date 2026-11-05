//! De-blind an answer sheet into per-backend, per-condition opinion scores.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use super::key::{ANSWERS_FILE, Key};
use super::{Result, bad};

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
	pub rating: Option<u8>,
	pub preferred: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
	pub backend: String,
	/// A condition such as `snr05`, or `all`.
	pub condition: String,
	pub rated: usize,
	pub mean_rating: Option<f64>,
	pub picked: usize,
	pub trials_with_pick: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
	pub seed: u64,
	pub clips: usize,
	pub clips_rated: usize,
	pub trials: usize,
	pub trials_with_pick: usize,
	pub rows: Vec<Row>,
}

pub fn parse_answers(text: &str) -> Result<BTreeMap<String, Answer>> {
	let mut lines = text.lines().filter(|l| !l.trim().is_empty());
	let header: Vec<&str> = lines
		.next()
		.unwrap_or("")
		.split(',')
		.map(str::trim)
		.collect();
	if header != ["clip", "rating", "preferred"] {
		return bad("answers.csv must start with the header: clip,rating,preferred");
	}
	let mut answers = BTreeMap::new();
	for line in lines {
		let cells: Vec<&str> = line.split(',').map(str::trim).collect();
		let [clip, rating, preferred] = cells[..] else {
			return bad(format!("expected 3 columns in: {line}"));
		};
		let rating = match rating {
			"" => None,
			r => match r.parse::<u8>() {
				Ok(v @ 1..=5) => Some(v),
				_ => return bad(format!("rating for {clip} must be 1-5, got {r:?}")),
			},
		};
		let preferred = match preferred.to_ascii_lowercase().as_str() {
			"" | "0" | "no" | "n" => false,
			"x" | "1" | "yes" | "y" => true,
			other => {
				return bad(format!(
					"preferred for {clip} must be x or empty, got {other:?}"
				));
			}
		};
		if answers
			.insert(clip.to_owned(), Answer { rating, preferred })
			.is_some()
		{
			return bad(format!("clip {clip} appears twice"));
		}
	}
	Ok(answers)
}

fn condition_rank(condition: &str) -> i64 {
	match condition.strip_prefix("snr") {
		Some(v) => match v.strip_prefix('m') {
			Some(n) => -n.parse::<i64>().unwrap_or(0),
			None => v.parse().unwrap_or(0),
		},
		None => i64::MAX,
	}
}

pub fn summarize(key: &Key, answers: &BTreeMap<String, Answer>) -> Result<Summary> {
	let known: BTreeSet<&str> = key.clips.iter().map(|c| c.clip.as_str()).collect();
	if let Some(unknown) = answers.keys().find(|c| !known.contains(c.as_str())) {
		return bad(format!("answer for unknown clip {unknown}"));
	}
	let mut picks: BTreeMap<usize, &str> = BTreeMap::new();
	for clip in &key.clips {
		if answers.get(&clip.clip).is_some_and(|a| a.preferred)
			&& let Some(first) = picks.insert(clip.trial, &clip.clip)
		{
			return bad(format!(
				"trial {} has two preferred clips ({first}, {})",
				clip.trial, clip.clip
			));
		}
	}
	let mut conditions: Vec<&str> = key.clips.iter().map(|c| c.condition.as_str()).collect();
	conditions.sort_by_key(|c| (condition_rank(c), c.to_string()));
	conditions.dedup();
	let mut rows = Vec::new();
	for backend in &key.backends {
		for condition in conditions.iter().copied().chain(["all"]) {
			let group: Vec<_> = key
				.clips
				.iter()
				.filter(|c| {
					&c.backend == backend && (condition == "all" || c.condition == condition)
				})
				.collect();
			let ratings: Vec<f64> = group
				.iter()
				.filter_map(|c| answers.get(&c.clip)?.rating.map(f64::from))
				.collect();
			let trials: BTreeSet<usize> = group.iter().map(|c| c.trial).collect();
			let trials_with_pick = trials.iter().filter(|t| picks.contains_key(t)).count();
			let picked = group
				.iter()
				.filter(|c| answers.get(&c.clip).is_some_and(|a| a.preferred))
				.count();
			rows.push(Row {
				backend: backend.clone(),
				condition: condition.to_owned(),
				rated: ratings.len(),
				mean_rating: (!ratings.is_empty())
					.then(|| ratings.iter().sum::<f64>() / ratings.len() as f64),
				picked,
				trials_with_pick,
			});
		}
	}
	let trials: BTreeSet<usize> = key.clips.iter().map(|c| c.trial).collect();
	Ok(Summary {
		seed: key.seed,
		clips: key.clips.len(),
		clips_rated: answers.values().filter(|a| a.rating.is_some()).count(),
		trials: trials.len(),
		trials_with_pick: picks.len(),
		rows,
	})
}

pub fn render_table(summary: &Summary) -> String {
	let mut text = format!(
		"seed {}; {} of {} clips rated; {} of {} trials have a preferred pick\n\n",
		summary.seed, summary.clips_rated, summary.clips, summary.trials_with_pick, summary.trials
	);
	text.push_str("backend | condition | rated | mean rating | picked / trials with a pick\n");
	for r in &summary.rows {
		let mean = r
			.mean_rating
			.map_or("n/a".to_owned(), |m| format!("{m:.2}"));
		text.push_str(&format!(
			"{} | {} | {} | {mean} | {} / {}\n",
			r.backend, r.condition, r.rated, r.picked, r.trials_with_pick
		));
	}
	text
}

pub fn score(session: &Path) -> Result<Summary> {
	let key = Key::load(session)?;
	let answers = parse_answers(&std::fs::read_to_string(session.join(ANSWERS_FILE))?)?;
	let summary = summarize(&key, &answers)?;
	let mut csv = String::from("backend,condition,rated,mean_rating,picked,trials_with_pick\n");
	for r in &summary.rows {
		let mean = r.mean_rating.map_or(String::new(), |m| format!("{m:.3}"));
		csv.push_str(&format!(
			"{},{},{},{mean},{},{}\n",
			r.backend, r.condition, r.rated, r.picked, r.trials_with_pick
		));
	}
	std::fs::write(session.join("summary.csv"), csv)?;
	Ok(summary)
}
