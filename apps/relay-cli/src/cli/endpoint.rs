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
	if let Some(profile_path) = std::env::var_os("RELAY_TCP_PROFILE") {
		return Ok(Endpoint::LoopbackTcp {
			profile_path: profile_path.into(),
		});
	}
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
#[cfg(unix)]
pub fn default_endpoint() -> Result<Endpoint, CliError> {
	let uid = relay_api::current_uid()?;
	let path = relay_api::default_socket_path(
		std::env::var_os("XDG_RUNTIME_DIR"),
		&std::env::temp_dir(),
		uid,
	);
	Ok(Endpoint::Unix(path))
}

/// The per-user default endpoint: a loopback profile in the temp directory.
#[cfg(not(unix))]
pub fn default_endpoint() -> Result<Endpoint, CliError> {
	let dir = std::env::temp_dir().join("relay");
	Ok(Endpoint::LoopbackTcp {
		profile_path: relay_api::profile_path(&dir),
	})
}

/// Human readable form for error messages.
pub fn describe(endpoint: &Endpoint) -> String {
	match endpoint {
		#[cfg(unix)]
		Endpoint::Unix(path) => path.display().to_string(),
		Endpoint::LoopbackTcp { profile_path } => {
			format!("tcp profile {}", profile_path.display())
		}
	}
}
