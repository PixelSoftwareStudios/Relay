//! `relay daemon`: host the engine in this process.

use relay_config::Paths;
use relay_engine::daemon::{RunOptions, Transport};
use tracing_subscriber::EnvFilter;

use crate::cli::{CliError, DaemonArgs, GlobalArgs};

pub async fn run(global: &GlobalArgs, args: &DaemonArgs) -> Result<(), CliError> {
	let _ = tracing_subscriber::fmt()
		.with_env_filter(
			EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
		)
		.try_init();
	let paths = Paths::platform().map_err(|e| CliError::Other(e.into()))?;
	let mut options = RunOptions::new(paths);
	options.transport = if args.tcp || global.tcp_profile.is_some() {
		Transport::LoopbackTcp(global.tcp_profile.clone())
	} else {
		Transport::Unix(global.socket.clone())
	};
	relay_engine::daemon::run(options)
		.await
		.map_err(|e| CliError::Other(e.into()))
}
