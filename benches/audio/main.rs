//! `audio-corpus generate <dir>` / `audio-corpus worker <backend> <seconds> [qos]\n  audio-corpus outliers <backend> <wav> <plain|qos>\n  audio-corpus stoi <clean.wav> <degraded.wav>\n  audio-corpus run <dir> [--backends a,b] [--out file.json] [--qos on]`

use std::path::PathBuf;
use std::process::ExitCode;

use relay_benches::audio::corpus::{discover, generate_synthetic};
use relay_benches::audio::runner::{default_backends, run};

fn usage() -> ExitCode {
	eprintln!(
		"usage:\n  audio-corpus generate <dir>\n  audio-corpus build-real <corpus-root> <out-dir>\n  audio-corpus run <dir> [--backends a,b,...] [--out file.json] [--qos on]"
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
		Some("outliers") if args.len() == 4 => relay_benches::audio::worker::outliers(
			&args[1],
			&PathBuf::from(&args[2]),
			args[3] == "qos",
		),
		Some("worker") if args.len() == 3 || args.len() == 4 => args[2]
			.parse::<f64>()
			.map_err(|e| relay_benches::audio::BenchError::Corpus(e.to_string()))
			.and_then(|seconds| {
				let report = relay_benches::audio::worker::run(
					&args[1],
					seconds,
					args.get(3).is_some_and(|a| a == "qos"),
				)?;
				println!("{}", serde_json::to_string_pretty(&report)?);
				Ok(())
			}),
		Some("stoi") if args.len() == 3 => {
			let rate = 48_000;
			(|| {
				let clean = relay_benches::audio::wav::read_mono_48k(&PathBuf::from(&args[1]))?;
				let degraded = relay_benches::audio::wav::read_mono_48k(&PathBuf::from(&args[2]))?;
				if let Some(s) = relay_benches::audio::stoi::stoi(&clean, &degraded, rate) {
					println!("{{\"stoi\": {:.6}, \"estoi\": {:.6}}}", s.stoi, s.estoi);
				}
				Ok(())
			})()
		}
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
			("--qos", Some(v)) if v == "on" => {
				relay_benches::audio::worker::request_interactive_qos();
			}
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
