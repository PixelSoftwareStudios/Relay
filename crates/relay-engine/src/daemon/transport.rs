//! Binding and serving the local API.

use std::path::PathBuf;
use std::sync::Arc;

use relay_api::{
	ApiError, ApiLimits, ClientValidator, Endpoint, TcpProfile, bind_loopback, profile_path,
	serve_tcp_service,
};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use super::Transport;
use super::error::DaemonError;

/// A bound listener and the files that must go away with it.
pub(super) enum Bound {
	#[cfg(unix)]
	Unix(relay_api::UdsListener, PathBuf),
	Tcp(TcpListener, TcpProfile, PathBuf),
}

fn default_dir() -> Result<PathBuf, DaemonError> {
	#[cfg(unix)]
	{
		let uid = relay_api::current_uid()?;
		let socket = relay_api::default_socket_path(
			std::env::var_os("XDG_RUNTIME_DIR"),
			&std::env::temp_dir(),
			uid,
		);
		Ok(socket
			.parent()
			.map_or_else(std::env::temp_dir, std::path::Path::to_path_buf))
	}
	#[cfg(not(unix))]
	{
		// The same place `relay_client::endpoint::default_endpoint` looks.
		Ok(std::env::temp_dir().join("relay"))
	}
}

pub(super) async fn bind(transport: &Transport) -> Result<Bound, DaemonError> {
	match transport {
		#[cfg(unix)]
		Transport::Unix(path) => {
			let path = match path {
				Some(path) => path.clone(),
				None => default_dir()?.join("engine.sock"),
			};
			let listener = relay_api::UdsListener::bind(&path).await?;
			Ok(Bound::Unix(listener, path))
		}
		#[cfg(not(unix))]
		Transport::Unix(_) => Err(DaemonError::Identity(
			"Unix sockets are not available here; use loopback TCP".to_owned(),
		)),
		Transport::LoopbackTcp(path) => {
			let path = match path {
				Some(path) => path.clone(),
				None => profile_path(&default_dir()?),
			};
			if let Some(dir) = path.parent() {
				#[cfg(unix)]
				relay_api::prepare_runtime_dir(dir)?;
				#[cfg(not(unix))]
				std::fs::create_dir_all(dir)?;
			}
			let (listener, profile) = bind_loopback(&path).await?;
			Ok(Bound::Tcp(listener, profile, path))
		}
	}
}

impl Bound {
	pub(super) fn endpoint(&self) -> Endpoint {
		match self {
			#[cfg(unix)]
			Self::Unix(_, path) => Endpoint::Unix(path.clone()),
			Self::Tcp(_, _, path) => Endpoint::LoopbackTcp {
				profile_path: path.clone(),
			},
		}
	}

	/// The file to remove when the engine stops.
	pub(super) fn leftover(&self) -> PathBuf {
		match self {
			#[cfg(unix)]
			Self::Unix(_, path) => path.clone(),
			Self::Tcp(_, _, path) => path.clone(),
		}
	}

	pub(super) fn serve(
		self,
		service: impl relay_api::EngineService,
		validator: Arc<dyn ClientValidator>,
		limits: ApiLimits,
		signal: impl Future<Output = ()> + Send + 'static,
	) -> JoinHandle<Result<(), ApiError>> {
		match self {
			#[cfg(unix)]
			Self::Unix(listener, _) => tokio::spawn(relay_api::serve_unix_service(
				service, listener, validator, limits, signal,
			)),
			Self::Tcp(listener, profile, _) => tokio::spawn(async move {
				serve_tcp_service(service, listener, &profile, validator, limits, signal).await
			}),
		}
	}
}
