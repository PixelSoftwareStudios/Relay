//! `relay daemon`: host the engine in this process.

use std::path::PathBuf;

use crate::cli::{CliError, DaemonArgs, GlobalArgs, endpoint};

/// What the hosted engine should do.
#[derive(Debug, Clone)]
pub struct DaemonOptions {
	/// Unix socket path to serve, when not using loopback TCP.
	pub socket: Option<PathBuf>,
	/// Serve loopback TCP with a token profile instead.
	pub tcp: bool,
	/// Explicit TCP profile path, if any.
	pub tcp_profile: Option<PathBuf>,
}

pub async fn run(global: &GlobalArgs, args: &DaemonArgs) -> Result<(), CliError> {
	let socket = match (&global.socket, args.tcp || global.tcp_profile.is_some()) {
		(Some(path), _) => Some(path.clone()),
		(None, true) => None,
		(None, false) => match endpoint::default_endpoint()? {
			#[cfg(unix)]
			relay_api::Endpoint::Unix(path) => Some(path),
			_ => None,
		},
	};
	let options = DaemonOptions {
		socket,
		tcp: args.tcp || global.tcp_profile.is_some(),
		tcp_profile: global.tcp_profile.clone(),
	};
	run_engine_host(options).await.map_err(CliError::Other)
}

/// Host the engine until interrupted.
///
/// Placeholder: this becomes a call to `relay_engine::daemon::run` once the
/// engine exposes it. Keeping the seam here means no other file changes.
pub async fn run_engine_host(options: DaemonOptions) -> anyhow::Result<()> {
	let _ = options;
	anyhow::bail!(
		"the engine host is not available in this build yet (relay-engine daemon::run is pending)"
	)
}
