//! Helpers that assemble a guarded tonic server over the supported transports.

use std::future::Future;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

use crate::middleware::{GuardConfig, GuardLayer};
use crate::pb::engine_server::{Engine, EngineServer};
use crate::registry::ClientValidator;
use crate::{ApiError, ApiLimits, TcpProfile};

/// Wrap an engine implementation with the message-size limits.
pub fn engine_server<E: Engine>(engine: E, limits: &ApiLimits) -> EngineServer<E> {
	EngineServer::new(engine)
		.max_decoding_message_size(limits.max_message_bytes)
		.max_encoding_message_size(limits.max_message_bytes)
}

fn guard(
	validator: Arc<dyn ClientValidator>,
	limits: &ApiLimits,
	profile: Option<&TcpProfile>,
) -> GuardLayer {
	GuardLayer::new(GuardConfig {
		validator,
		token: profile.map(|p| Arc::from(p.token.as_str())),
		max_message_bytes: limits.max_message_bytes,
		max_in_flight: limits.max_in_flight,
	})
}

/// Serve over an owner-only Unix socket until `shutdown` resolves.
#[cfg(unix)]
pub async fn serve_unix<E: Engine>(
	engine: E,
	listener: crate::UdsListener,
	validator: Arc<dyn ClientValidator>,
	limits: ApiLimits,
	shutdown: impl Future<Output = ()>,
) -> Result<(), ApiError> {
	Server::builder()
		.layer(guard(validator, &limits, None))
		.add_service(engine_server(engine, &limits))
		.serve_with_incoming_shutdown(listener.incoming(), shutdown)
		.await?;
	Ok(())
}

/// Serve over loopback TCP, requiring the profile's bearer token on every RPC.
pub async fn serve_tcp<E: Engine>(
	engine: E,
	listener: TcpListener,
	profile: &TcpProfile,
	validator: Arc<dyn ClientValidator>,
	limits: ApiLimits,
	shutdown: impl Future<Output = ()>,
) -> Result<(), ApiError> {
	Server::builder()
		.layer(guard(validator, &limits, Some(profile)))
		.add_service(engine_server(engine, &limits))
		.serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown)
		.await?;
	Ok(())
}
