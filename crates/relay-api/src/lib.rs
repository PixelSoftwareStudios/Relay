//! Local control API for the Relay engine.
//!
//! Generated `relay.v1` messages stay in [`pb`] and never leak domain types;
//! conversion to validated domain values belongs to the engine crate.

mod client;
mod client_ext;
mod client_messages;
mod error;
mod limits;
mod middleware;
mod redact;
mod registry;
mod server;
mod tcp;
mod watch;

#[cfg(unix)]
mod uds;

/// Generated protobuf messages and gRPC client/server stubs.
#[allow(clippy::all, clippy::pedantic, missing_docs)]
pub mod pb {
	tonic::include_proto!("relay.v1");
}

pub use client::{Endpoint, RelayClient};
pub use client_messages::{
	EXPORT_INLINE_MAX_BYTES, SessionItem, SessionTracker, SessionWatch, entry_text,
};
pub use error::ApiError;
pub use limits::{AVATAR_MAX_BYTES, ApiLimits};
pub use middleware::{GuardConfig, GuardLayer, SENSITIVE_METHODS, StreamGuard, StreamLimiter};
pub use registry::{ClientId, ClientRegistry, ClientValidator};
pub use server::{engine_server, serve_tcp};
pub use tcp::{TcpProfile, bind_loopback, profile_path};
pub use watch::{BoundedWatch, StateTracker, StateWatch, WatchItem, WatchOptions};

#[cfg(unix)]
pub use server::serve_unix;
#[cfg(unix)]
pub use uds::{UdsListener, check_peer, current_uid, default_socket_path, prepare_runtime_dir};

/// Metadata key carrying the hex-encoded 16-byte client id.
pub const CLIENT_ID_HEADER: &str = "relay-client-id";
/// Metadata key carrying the hex-encoded engine incarnation the client knows.
pub const INCARNATION_HEADER: &str = "relay-incarnation";
/// Metadata key carrying the loopback-TCP bearer token.
pub const AUTHORIZATION_HEADER: &str = "authorization";
