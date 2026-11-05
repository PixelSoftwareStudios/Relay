use std::io;
use std::path::PathBuf;

/// Failures from the API transport helpers and the client wrapper.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
	/// Socket or file operation failed.
	#[error("i/o error: {0}")]
	Io(#[from] io::Error),
	/// gRPC channel could not be established.
	#[error("transport error: {0}")]
	Transport(#[from] tonic::transport::Error),
	/// The engine answered with a gRPC status.
	#[error("engine returned status: {0}")]
	Status(Box<tonic::Status>),
	/// Another live engine already owns the endpoint.
	#[error("endpoint {0} is already served by a live engine")]
	AlreadyRunning(PathBuf),
	/// The endpoint path exists but is not a socket.
	#[error("{0} exists and is not a socket")]
	NotASocket(PathBuf),
	/// The path is owned by another user or is group/world accessible.
	#[error("{path} is not private to this user: {reason}")]
	InsecurePath {
		/// Offending path.
		path: PathBuf,
		/// What check failed.
		reason: &'static str,
	},
	/// A connecting peer belongs to another OS user.
	#[error("peer uid {peer} does not match engine uid {expected}")]
	PeerRejected {
		/// Peer's uid.
		peer: u32,
		/// Engine's uid.
		expected: u32,
	},
	/// TCP profile file is malformed or non-loopback.
	#[error("invalid tcp profile: {0}")]
	BadProfile(&'static str),
	/// Operation requires a registered client.
	#[error("client is not registered")]
	NotRegistered,
	/// Watch stream skipped revisions; the caller must resync.
	#[error("revision gap: expected {expected}, received {received}")]
	RevisionGap {
		/// Next revision that should have arrived.
		expected: u64,
		/// Revision that actually arrived.
		received: u64,
	},
	/// Watch stream is not strictly increasing.
	#[error("revision {received} is not newer than {last}")]
	StaleRevision {
		/// Last accepted revision.
		last: u64,
		/// Offending revision.
		received: u64,
	},
	/// The engine restarted; discard cached state.
	#[error("engine incarnation changed")]
	IncarnationChanged,
	/// The local subscription queue filled up.
	#[error("subscription queue overflowed; resync required")]
	SubscriptionOverflow,
	/// A watch stream ended or carried an empty envelope.
	#[error("watch stream ended or was malformed")]
	StreamClosed,
	/// Operation did not finish within the polling deadline.
	#[error("operation did not complete in time")]
	OperationTimeout,
	/// Response lacked a required field.
	#[error("malformed response: {0}")]
	InvalidResponse(&'static str),
}

impl From<tonic::Status> for ApiError {
	fn from(status: tonic::Status) -> Self {
		Self::Status(Box::new(status))
	}
}
