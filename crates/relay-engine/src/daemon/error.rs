//! Daemon start-up and shutdown failures.

use thiserror::Error;

use crate::service::ServiceError;

/// Why the daemon could not start or stop cleanly.
#[derive(Debug, Error)]
pub enum DaemonError {
	/// Preferences, trust, identity or avatar storage failed.
	#[error("configuration: {0}")]
	Config(#[from] relay_config::ConfigError),
	/// The local API endpoint could not be bound or served.
	#[error("local api: {0}")]
	Api(#[from] relay_api::ApiError),
	/// The control service rejected its configuration.
	#[error("service: {0}")]
	Service(#[from] ServiceError),
	/// The peer manager could not start.
	#[error("peers: {0}")]
	Peers(#[from] crate::peers::PeerError),
	/// The media system could not start.
	#[error("media: {0}")]
	Media(std::io::Error),
	/// The identity key could not be created or read.
	#[error("identity: {0}")]
	Identity(String),
	/// The channel driver could not load the stored channels.
	#[error("channels: {0}")]
	Channels(String),
	/// Signal handling or another OS facility failed.
	#[error("i/o: {0}")]
	Io(#[from] std::io::Error),
}
