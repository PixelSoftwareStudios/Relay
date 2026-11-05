use std::collections::BTreeMap;
use std::path::Path;

use relay_benches::audio::corpus::generate_synthetic;
use relay_benches::audio::synth::Rng;
use relay_benches::listening::key::{ANSWERS_FILE, CLIP_DIR, ClipKey, Key};
use relay_benches::listening::prepare::{Options, match_gain, prepare, remove_delay};
use relay_benches::listening::score::{Answer, parse_answers, score, summarize};
use relay_benches::listening::select::{shuffle, split_name};

fn options(corpus: &Path, out: &Path, seed: u64) -> Options {
	Options {
		corpus: corpus.to_path_buf(),
		out: out.to_path_buf(),
		backends: vec!["off".to_owned()],
		utterances: 3,
		conditions: vec!["snr00".to_owned(), "snr10".to_owned()],
		seed,
	}
}

fn clip(clip: &str, trial: usize, condition: &str, backend: &str) -> ClipKey {
	ClipKey {
		clip: clip.to_owned(),
		trial,
		utterance: "u".to_owned(),
		condition: condition.to_owned(),
		backend: backend.to_owned(),
		gain_db: 0.0,
		delay_removed_samples: 0,
	}
}

fn two_trial_key() -> Key {
	Key {
		schema_version: 1,
		seed: 7,
		backends: vec!["alpha".to_owned(), "beta".to_owned()],
		clips: vec![
			clip("trial01_A", 1, "snr00", "alpha"),
			clip("trial01_B", 1, "snr00", "beta"),
			clip("trial02_A", 2, "snr10", "beta"),
			clip("trial02_B", 2, "snr10", "alpha"),
		],
	}
}

fn answers(rows: &[(&str, Option<u8>, bool)]) -> BTreeMap<String, Answer> {
	rows.iter()
		.map(|&(c, rating, preferred)| (c.to_owned(), Answer { rating, preferred }))
		.collect()
}

#[test]
fn names_split_into_utterance_and_condition() {
	assert_eq!(
		split_name("u00-1272-128104-0010_snr05_kitchen"),
		Some(("u00-1272-128104-0010".to_owned(), "snr05".to_owned()))
	);
	assert_eq!(
		split_name("u01-x_clean"),
		Some(("u01-x".to_owned(), "clean".to_owned()))
	);
	assert_eq!(split_name("nothing"), None);
}

#[test]
fn shuffle_is_seeded_and_a_permutation() {
	let run = |seed| {
		let mut v: Vec<u32> = (0..20).collect();
		shuffle(&mut v, &mut Rng::new(seed));
		v
	};
	assert_eq!(run(5), run(5));
	assert_ne!(run(5), run(6));
	let mut sorted = run(5);
	sorted.sort_unstable();
	assert_eq!(sorted, (0..20).collect::<Vec<_>>());
}

#[test]
fn gain_matches_loudness_within_limits() {
	let reference = vec![0.1_f32; 100];
	let quiet = vec![0.05_f32; 100];
	assert!((match_gain(&reference, &quiet) - 2.0).abs() < 1e-9);
	let tiny = vec![0.0001_f32; 100];
	assert!(match_gain(&reference, &tiny) <= 10_f64.powf(0.6) + 1e-9);
	let peaky = vec![0.9_f32; 100];
	let loud_ref = vec![2.0_f32; 100];
	assert!(f64::from(0.9_f32) * match_gain(&loud_ref, &peaky) <= 0.98 + 1e-6);
	assert_eq!(match_gain(&reference, &[0.0; 10]), 1.0);
}

#[test]
fn delay_removal_keeps_length() {
	assert_eq!(remove_delay(&[0.0, 0.0, 1.0, 2.0], 2), [1.0, 2.0, 0.0, 0.0]);
	assert_eq!(remove_delay(&[1.0, 2.0], -3), [1.0, 2.0]);
}

#[test]
fn session_is_deterministic_and_never_leaks_labels() {
	let corpus = tempfile::tempdir().unwrap();
	generate_synthetic(corpus.path(), 1.5).unwrap();
	let (a, b, c) = (
		tempfile::tempdir().unwrap(),
		tempfile::tempdir().unwrap(),
		tempfile::tempdir().unwrap(),
	);
	let ka = prepare(&options(corpus.path(), a.path(), 11)).unwrap();
	let kb = prepare(&options(corpus.path(), b.path(), 11)).unwrap();
	let kc = prepare(&options(corpus.path(), c.path(), 12)).unwrap();
	assert_eq!(ka, kb, "same seed must reproduce the session");
	assert_ne!(
		ka.clips.iter().map(|c| &c.utterance).collect::<Vec<_>>(),
		kc.clips.iter().map(|c| &c.utterance).collect::<Vec<_>>()
	);
	assert_eq!(ka.clips.len(), 6);
	assert_eq!(ka.seed, 11);

	// Everything outside key/ is free of backend names and of the corpus
	// utterance names.
	let mut checked = 0;
	for entry in std::fs::read_dir(a.path().join(CLIP_DIR)).unwrap() {
		let name = entry.unwrap().file_name().into_string().unwrap();
		let stem = name.strip_suffix(".wav").unwrap();
		let (trial, letter) = stem.split_once('_').unwrap();
		assert!(trial.starts_with("trial") && letter.len() == 1, "{name}");
		checked += 1;
	}
	assert_eq!(checked, 6);
	let sheet = std::fs::read_to_string(a.path().join(ANSWERS_FILE)).unwrap();
	for secret in ["off", "white", "brown", "snr", "clean"] {
		assert!(!sheet.contains(secret), "answers.csv leaks {secret}");
	}
	assert!(a.path().join("key/key.json").exists());
}

#[test]
fn off_clips_are_unprocessed_copies() {
	let corpus = tempfile::tempdir().unwrap();
	generate_synthetic(corpus.path(), 1.5).unwrap();
	let out = tempfile::tempdir().unwrap();
	let key = prepare(&options(corpus.path(), out.path(), 3)).unwrap();
	assert!(key.clips.iter().all(|c| c.delay_removed_samples == 0));
	assert!(key.clips.iter().all(|c| c.gain_db.abs() < 1e-6));
}

#[test]
fn scoring_deblinds_means_and_picks() {
	let key = two_trial_key();
	let a = answers(&[
		("trial01_A", Some(2), false),
		("trial01_B", Some(4), true),
		("trial02_A", Some(5), true),
		("trial02_B", Some(3), false),
	]);
	let summary = summarize(&key, &a).unwrap();
	let row = |backend: &str, condition: &str| {
		summary
			.rows
			.iter()
			.find(|r| r.backend == backend && r.condition == condition)
			.unwrap()
	};
	assert_eq!(row("alpha", "snr00").mean_rating, Some(2.0));
	assert_eq!(row("beta", "snr00").mean_rating, Some(4.0));
	assert_eq!(row("beta", "snr10").mean_rating, Some(5.0));
	assert_eq!(row("alpha", "all").mean_rating, Some(2.5));
	assert_eq!(row("beta", "all").picked, 2);
	assert_eq!(row("alpha", "all").picked, 0);
	assert_eq!((summary.trials, summary.trials_with_pick), (2, 2));
}

#[test]
fn partial_answers_are_reported_not_invented() {
	let key = two_trial_key();
	let a = answers(&[("trial01_A", Some(3), false)]);
	let summary = summarize(&key, &a).unwrap();
	assert_eq!(summary.clips_rated, 1);
	let beta = summary
		.rows
		.iter()
		.find(|r| r.backend == "beta" && r.condition == "all")
		.unwrap();
	assert_eq!((beta.rated, beta.mean_rating), (0, None));
}

#[test]
fn bad_sheets_are_rejected() {
	let key = two_trial_key();
	assert!(parse_answers("clip,score\n").is_err());
	assert!(parse_answers("clip,rating,preferred\ntrial01_A,6,\n").is_err());
	assert!(parse_answers("clip,rating,preferred\ntrial01_A,x,\n").is_err());
	assert!(parse_answers("clip,rating,preferred\ntrial01_A,3,\ntrial01_A,2,\n").is_err());
	let two_picks = answers(&[("trial01_A", None, true), ("trial01_B", None, true)]);
	assert!(summarize(&key, &two_picks).is_err());
	let unknown = answers(&[("trial09_A", Some(3), false)]);
	assert!(summarize(&key, &unknown).is_err());
	let ok = parse_answers("clip,rating,preferred\ntrial01_A, 3 , X\ntrial01_B,,\n").unwrap();
	assert_eq!(
		ok["trial01_A"],
		Answer {
			rating: Some(3),
			preferred: true
		}
	);
}

#[test]
fn score_reads_the_session_files_and_writes_a_csv() {
	let dir = tempfile::tempdir().unwrap();
	two_trial_key().save(dir.path()).unwrap();
	std::fs::write(
		dir.path().join(ANSWERS_FILE),
		"clip,rating,preferred\ntrial01_A,1,\ntrial01_B,5,x\n",
	)
	.unwrap();
	let summary = score(dir.path()).unwrap();
	assert_eq!(summary.clips_rated, 2);
	let csv = std::fs::read_to_string(dir.path().join("summary.csv")).unwrap();
	assert!(csv.contains("beta,snr00,1,5.000,1,1"));
}
