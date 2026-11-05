//! `audio-corpus generate <dir>` / `audio-corpus worker <backend> <seconds>\n  audio-corpus run <dir> [--backends a,b] [--out file.json]`

use std::path::PathBuf;
use std::process::ExitCode;

use relay_benches::audio::corpus::{discover, generate_synthetic};
use relay_benches::audio::runner::{default_backends, run};

fn usage() -> ExitCode {
	eprintln!(
		"usage:\n  audio-corpus generate <dir>\n  audio-corpus build-real <corpus-root> <out-dir>\n  audio-corpus run <dir> [--backends a,b,...] [--out file.json]"
	);
	ExitCode::from(2)
}

fn main() -> ExitCode {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let result = match args.first().map(String::as_str) {
		Some("generate") if args.len() == 2 => generate_synthetic(&PathBuf::from(&args[1]), 8.0)
			.map(|p| {
				eprintln!("wrote {} pairs", p.len());
			}),
		Some("build-real") if args.len() == 3 => {
			relay_benches::audio::real::build(&PathBuf::from(&args[1]), &PathBuf::from(&args[2]))
				.map(|hash| println!("manifest sha256 {hash}"))
		}
		Some("worker") if args.len() == 3 => args[2]
			.parse::<f64>()
			.map_err(|e| relay_benches::audio::BenchError::Corpus(e.to_string()))
			.and_then(|seconds| {
				let report = relay_benches::audio::worker::run(&args[1], seconds)?;
				println!("{}", serde_json::to_string_pretty(&report)?);
				Ok(())
			}),
		Some("run") if args.len() >= 2 => run_command(&args[1..]),
		_ => return usage(),
	};
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(error) => {
			eprintln!("error: {error}");
			ExitCode::FAILURE
		}
	}
}

fn run_command(args: &[String]) -> relay_benches::audio::Result<()> {
	let dir = PathBuf::from(&args[0]);
	let mut backends = default_backends();
	let mut out: Option<PathBuf> = None;
	let mut rest = args[1..].iter();
	while let Some(flag) = rest.next() {
		match (flag.as_str(), rest.next()) {
			("--backends", Some(v)) => backends = v.split(',').map(str::to_owned).collect(),
			("--out", Some(v)) => out = Some(PathBuf::from(v)),
			_ => {
				return Err(relay_benches::audio::BenchError::Corpus(format!(
					"unexpected argument {flag}"
				)));
			}
		}
	}
	let pairs = discover(&dir)?;
	let report = run(&backends, &pairs)?;
	let json = serde_json::to_string_pretty(&report)?;
	match out {
		Some(path) => std::fs::write(path, json)?,
		None => println!("{json}"),
	}
	Ok(())
}
