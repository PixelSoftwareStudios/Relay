//! Resolving which engine endpoint to talk to.

use relay_api::Endpoint;

use super::{CliError, GlobalArgs};

/// Pick the endpoint from `--tcp-profile`, `--socket` or the per-user default.
pub fn resolve(global: &GlobalArgs) -> Result<Endpoint, CliError> {
	if let Some(profile_path) = &global.tcp_profile {
		return Ok(Endpoint::LoopbackTcp {
			profile_path: profile_path.clone(),
		});
	}
	if let Some(path) = &global.socket {
		return unix(path.clone());
	}
	if let Some(endpoint) = relay_client::endpoint::from_env() {
		return Ok(endpoint);
	}
	#[cfg(not(unix))]
	if let Some(path) = std::env::var_os("RELAY_SOCKET") {
		return unix(path.into());
	}
	default_endpoint()
}

#[cfg(unix)]
fn unix(path: std::path::PathBuf) -> Result<Endpoint, CliError> {
	Ok(Endpoint::Unix(path))
}

#[cfg(not(unix))]
fn unix(_: std::path::PathBuf) -> Result<Endpoint, CliError> {
	Err(CliError::Usage(
		"--socket needs a Unix platform; use --tcp-profile".into(),
	))
}

/// The per-user default endpoint. Needs a tokio runtime on Unix.
pub fn default_endpoint() -> Result<Endpoint, CliError> {
	Ok(relay_client::endpoint::default_endpoint()?)
}

pub use relay_client::endpoint::describe;
