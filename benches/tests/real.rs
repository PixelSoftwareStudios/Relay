//! Needs the local, gitignored corpus under `benches/corpus`; run with
//! `cargo test --release -p relay-benches --test real -- --ignored`.

use std::path::PathBuf;

use relay_benches::audio::corpus::discover;
use relay_benches::audio::real::{SNRS_DB, UTTERANCES, build};
use relay_benches::audio::wav::read_mono;

fn root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

#[test]
#[ignore = "needs the local corpus"]
fn flac_decodes_at_16k() {
	let file = root()
		.join("librispeech/extracted/LibriSpeech/dev-clean/2035/147960/2035-147960-0008.flac");
	let (samples, rate) = read_mono(&file).unwrap();
	assert_eq!(rate, 16_000);
	assert!(samples.len() > 16_000 && samples.iter().all(|s| s.abs() <= 1.0));
}

#[test]
#[ignore = "needs the local corpus"]
fn builder_is_deterministic_and_complete() {
	let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
	let ha = build(&root(), a.path()).unwrap();
	let hb = build(&root(), b.path()).unwrap();
	assert_eq!(ha, hb, "same inputs must give the same manifest");
	let pairs = discover(a.path()).unwrap();
	assert_eq!(pairs.len(), UTTERANCES * (SNRS_DB.len() + 1));
	let first = std::fs::read(&pairs[0].noisy).unwrap();
	let again = std::fs::read(
		b.path()
			.join("noisy")
			.join(pairs[0].noisy.file_name().unwrap()),
	)
	.unwrap();
	assert_eq!(first, again);
}
