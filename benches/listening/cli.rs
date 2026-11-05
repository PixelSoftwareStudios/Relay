//! `audio-corpus listen prepare|score`.

use std::path::PathBuf;

use super::prepare::{Options, prepare};
use super::score::{render_table, score};
use super::{Result, bad};
use crate::audio::runner::make_candidate;

pub const USAGE: &str = "  audio-corpus listen prepare <corpus-dir> [--out DIR] [--backends a,b,...] [--utterances N] [--conditions snr00,snr10,clean] [--seed N]\n  audio-corpus listen score <session-dir>";

/// Backends worth comparing by ear that this build and the local weights
/// can run; the ones that cannot are named on stderr.
pub fn default_backends() -> Vec<String> {
	const PREFERRED: [&str; 7] = [
		"off",
		"sonora-ns",
		"nnnoiseless",
		"gtcrn",
		"dtln",
		"fastenhancer-s",
		"fastenhancer-m",
	];
	PREFERRED
		.into_iter()
		.filter(|name| match make_candidate(name) {
			Ok(_) => true,
			Err(error) => {
				eprintln!("skipping {name}: {error}");
				false
			}
		})
		.map(str::to_owned)
		.collect()
}

pub fn parse_prepare(args: &[String]) -> Result<Options> {
	let Some(corpus) = args.first() else {
		return bad("missing <corpus-dir>");
	};
	let mut seed: Option<u64> = None;
	let mut options = Options {
		corpus: PathBuf::from(corpus),
		out: PathBuf::new(),
		backends: default_backends(),
		utterances: 4,
		conditions: ["snr00", "snr10", "clean"].map(str::to_owned).to_vec(),
		seed: 0,
	};
	let mut out: Option<PathBuf> = None;
	let mut rest = args[1..].iter();
	while let Some(flag) = rest.next() {
		let Some(value) = rest.next() else {
			return bad(format!("{flag} needs a value"));
		};
		let list = || value.split(',').map(str::to_owned).collect::<Vec<_>>();
		match flag.as_str() {
			"--out" => out = Some(PathBuf::from(value)),
			"--backends" => options.backends = list(),
			"--conditions" => options.conditions = list(),
			"--utterances" => {
				options.utterances = value.parse().or_else(|_| bad("--utterances is a number"))?;
			}
			"--seed" => seed = Some(value.parse().or_else(|_| bad("--seed is a number"))?),
			other => return bad(format!("unexpected argument {other}")),
		}
	}
	options.seed = seed.unwrap_or_else(|| {
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map_or(1, |d| d.as_nanos() as u64)
	});
	options.out = out.unwrap_or_else(|| {
		PathBuf::from("benches/listening-out").join(format!("session-{}", options.seed))
	});
	Ok(options)
}

pub fn run(args: &[String]) -> Result<()> {
	match args.first().map(String::as_str) {
		Some("prepare") => {
			let options = parse_prepare(&args[1..])?;
			let key = prepare(&options)?;
			println!(
				"wrote {} clips ({} trials) to {}\nseed {}\nlisten to <out>/clips/*.wav, fill <out>/answers.csv, then run: audio-corpus listen score {}",
				key.clips.len(),
				key.clips.iter().map(|c| c.trial).max().unwrap_or(0),
				options.out.display(),
				key.seed,
				options.out.display()
			);
			Ok(())
		}
		Some("score") if args.len() == 2 => {
			print!("{}", render_table(&score(&PathBuf::from(&args[1]))?));
			Ok(())
		}
		_ => bad(format!("usage:\n{USAGE}")),
	}
}
