//! `relayd`: the Relay engine as its own per-user process.
//!
//! It owns the audio devices, the identity, the peers and the calls, and it serves the local
//! API. Clients (`relay`, the desktop app) never run it in their own process: they start it
//! detached, as a separate process, by running their own executable with
//! [`relay_io::proc::RUN_DAEMON_ARG`] first. Every client binary calls [`exit_if_requested`] as
//! the first thing in `main`, so a daemon started by a client is the client's own binary, with
//! its code identity, and nothing searches for a `relayd` file. The `relayd` binary is the same
//! entry for service units and headless machines.

mod args;

use std::ffi::OsString;

use clap::Parser;
use relay_io::proc::RUN_DAEMON_ARG;
use tracing_subscriber::EnvFilter;

use args::Args;

/// Runs the daemon and exits the process when this one was started with [`RUN_DAEMON_ARG`] as
/// its first argument (the remaining arguments are `relayd`'s); returns at once for a normal
/// start of the client. Call it before anything else in `main`.
pub fn exit_if_requested() {
	let mut argv = std::env::args_os();
	if argv.nth(1).is_some_and(|arg| arg == RUN_DAEMON_ARG) {
		let code = run(std::iter::once(OsString::from("relayd")).chain(argv));
		std::process::exit(code);
	}
}

/// Parses `relayd`'s command line (the first item is the program name) and runs the engine
/// until it stops. Returns the process exit code.
pub fn run<I>(argv: I) -> i32
where
	I: IntoIterator<Item = OsString>,
{
	let args = Args::parse_from(argv);
	init_logging(&args.log_level);
	let runtime = match tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
	{
		Ok(runtime) => runtime,
		Err(error) => {
			eprintln!("relayd: cannot start the async runtime: {error}");
			return 1;
		}
	};
	let result = args
		.options()
		.map(|options| runtime.block_on(relay_engine::daemon::run(options)));
	// A keychain prompt that is still open holds a blocking thread; do not wait for the user.
	runtime.shutdown_timeout(std::time::Duration::from_secs(1));
	match result {
		Ok(Ok(())) => 0,
		Ok(Err(error)) => {
			tracing::error!(%error, "engine stopped with an error");
			eprintln!("relayd: {error}");
			1
		}
		Err(error) => {
			eprintln!("relayd: {error:#}");
			2
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
