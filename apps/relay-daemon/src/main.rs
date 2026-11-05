//! `relayd`: the Relay engine as its own per-user process.
//!
//! It owns the audio devices, the identity, the peers and the calls, and it
//! serves the local API. Clients (`relay`, the desktop app) never embed it;
//! they find it, start it and stop it through its endpoint.

mod args;

use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use args::Args;

fn main() -> ExitCode {
	let args = Args::parse();
	init_logging(&args.log_level);
	let runtime = match tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
	{
		Ok(runtime) => runtime,
		Err(error) => {
			eprintln!("relayd: cannot start the async runtime: {error}");
			return ExitCode::FAILURE;
		}
	};
	let result = args
		.options()
		.map(|options| runtime.block_on(relay_engine::daemon::run(options)));
	match result {
		Ok(Ok(())) => ExitCode::SUCCESS,
		Ok(Err(error)) => {
			tracing::error!(%error, "engine stopped with an error");
			eprintln!("relayd: {error}");
			ExitCode::FAILURE
		}
		Err(error) => {
			eprintln!("relayd: {error:#}");
			ExitCode::from(2)
		}
	}
}

fn init_logging(level: &str) {
	let filter = EnvFilter::try_from_env("RELAY_LOG")
		.or_else(|_| EnvFilter::try_new(level))
		.unwrap_or_else(|_| EnvFilter::new("info"));
	let _ = tracing_subscriber::fmt()
		.with_env_filter(filter)
		.with_writer(std::io::stderr)
		.with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
		.try_init();
}
