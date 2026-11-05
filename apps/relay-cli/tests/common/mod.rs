//! Test doubles: a scripted `Control` and an in-process engine over a real socket.

#![allow(dead_code, unused_imports)]

pub mod control;
pub mod engine;
pub mod fixtures;

pub use control::FakeControl;

/// Run a command line against a `Control` and return its stdout.
pub async fn run_with<C: relay_cli::client::Control + relay_cli::client::Subscribe>(
	client: &mut C,
	args: &[&str],
	json: bool,
) -> Result<String, relay_cli::cli::CliError> {
	use clap::Parser;
	let mut argv = vec!["relay"];
	if json {
		argv.push("--json");
	}
	argv.extend_from_slice(args);
	let cli = relay_cli::cli::Cli::try_parse_from(argv).expect("valid arguments");
	let mut buffer = Vec::new();
	let mut out = relay_cli::commands::Out::new(json, &mut buffer);
	relay_cli::commands::execute(client, cli.command, &mut out).await?;
	Ok(String::from_utf8(buffer).expect("utf8 output"))
}
