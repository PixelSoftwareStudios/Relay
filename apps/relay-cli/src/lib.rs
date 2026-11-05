//! The `relay` command line: scriptable commands, a terminal UI and the
//! engine host. Everything talks to the engine through [`client::Control`],
//! so commands and screens are testable against a fake.

pub mod cli;
pub mod client;
pub mod commands;
pub mod tui;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, CliError};

/// Parse `std::env::args`, run the command and map the result to an exit code.
pub fn main_exit() -> ExitCode {
	let cli = Cli::parse();
	let runtime = match tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
	{
		Ok(runtime) => runtime,
		Err(e) => {
			eprintln!("relay: cannot start async runtime: {e}");
			return ExitCode::from(cli::exit::FAILURE);
		}
	};
	let json = cli.global.json;
	match runtime.block_on(commands::run(cli, &mut std::io::stdout())) {
		Ok(()) => ExitCode::SUCCESS,
		Err(err) => {
			report(&err, json);
			ExitCode::from(err.exit_code())
		}
	}
}

fn report(err: &CliError, json: bool) {
	if json {
		let body =
			serde_json::json!({ "error": { "code": err.code(), "message": err.to_string() } });
		eprintln!("{body}");
	} else {
		eprintln!("relay: {err}");
	}
}
