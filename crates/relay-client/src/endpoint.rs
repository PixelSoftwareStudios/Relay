//! Choosing the engine endpoint. Explicit choices (flags) belong to the app;
//! the environment and per-user default are shared.

use std::ffi::OsString;

use relay_api::{ApiError, Endpoint};

/// Endpoint named by `RELAY_TCP_PROFILE`, or on Unix `RELAY_SOCKET`.
pub fn from_env() -> Option<Endpoint> {
	from_vars(|name| std::env::var_os(name))
}

/// [`from_env`] with an injectable variable lookup.
pub fn from_vars(get: impl Fn(&str) -> Option<OsString>) -> Option<Endpoint> {
	if let Some(profile_path) = get("RELAY_TCP_PROFILE") {
		return Some(Endpoint::LoopbackTcp {
			profile_path: profile_path.into(),
		});
	}
	#[cfg(unix)]
	if let Some(path) = get("RELAY_SOCKET") {
		return Some(Endpoint::Unix(path.into()));
	}
	None
}

/// The per-user default endpoint. Needs a tokio runtime on Unix.
#[cfg(unix)]
pub fn default_endpoint() -> Result<Endpoint, ApiError> {
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
pub fn default_endpoint() -> Result<Endpoint, ApiError> {
	let dir = std::env::temp_dir().join("relay");
	Ok(Endpoint::LoopbackTcp {
		profile_path: relay_api::profile_path(&dir),
	})
}

/// Environment choice, else the per-user default.
pub fn resolve() -> Result<Endpoint, ApiError> {
	match from_env() {
		Some(endpoint) => Ok(endpoint),
		None => default_endpoint(),
	}
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
