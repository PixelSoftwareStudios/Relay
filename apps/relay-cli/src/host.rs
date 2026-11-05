//! Process setup: a plain tokio runtime for most commands, and for the ones
//! that register a global hotkey a main thread that starts the backend and
//! services the platform loop while tokio runs on worker threads (macOS needs
//! the hotkey manager and run loop on the main thread).

use clap::Parser;
use relay_platform::hotkey::{detect_backend, run_main_loop};
use std::process::ExitCode;
use tokio::runtime::Runtime;

use crate::cli::{Cli, CliError};
use crate::{commands, hotkey};

/// Parse `std::env::args`, run the command and map the result to an exit code.
pub fn main_exit() -> ExitCode {
	let cli = Cli::parse();
	let json = cli.global.json;
	let result = if cli.command.uses_hotkeys() {
		run_with_main_loop(cli)
	} else {
		run_plain(cli)
	};
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(err) => {
			report(&err, json);
			ExitCode::from(err.exit_code())
		}
	}
}

fn runtime() -> Result<Runtime, CliError> {
	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.map_err(CliError::Io)
}

fn run_plain(cli: Cli) -> Result<(), CliError> {
	runtime()?.block_on(commands::run(cli, &mut std::io::stdout()))
}

fn run_with_main_loop(cli: Cli) -> Result<(), CliError> {
	let runtime = runtime()?;
	let accelerator = hotkey::configured_binding(cli.command.binding_override());
	// The backend is created, started and stopped on this (the main) thread.
	let mut backend = detect_backend();
	let feed = hotkey::start_with(backend.as_mut(), &accelerator);
	let worker = std::thread::Builder::new()
		.name("relay-main".into())
		.spawn(move || runtime.block_on(commands::run_with(cli, feed, &mut std::io::stdout())))
		.map_err(CliError::Io)?;
	run_main_loop(|| !worker.is_finished());
	backend.stop();
	worker
		.join()
		.unwrap_or_else(|_| Err(CliError::Other(anyhow::anyhow!("the command panicked"))))
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
