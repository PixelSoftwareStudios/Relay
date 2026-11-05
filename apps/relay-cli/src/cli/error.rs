//! The error type every command returns.

use relay_api::ApiError;
use relay_api::pb::Problem;

use super::exit;

/// Why a lookup of a peer, call or device failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetError {
	/// Nothing matched.
	#[error("no {kind} matches `{query}`")]
	NotFound {
		/// What was being looked up.
		kind: &'static str,
		/// What the user typed.
		query: String,
	},
	/// More than one candidate matched.
	#[error("`{query}` is ambiguous; candidates: {candidates}")]
	Ambiguous {
		/// What the user typed.
		query: String,
		/// Comma separated candidate labels.
		candidates: String,
	},
	/// A call was needed but none, or several, are active and none was named.
	#[error("{0}")]
	NoDefault(String),
}

/// Failure of a CLI command.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
	/// Connecting or registering failed.
	#[error(
		"cannot reach the engine at {endpoint}: {source}; if the daemon is not running, try `relay daemon start`"
	)]
	Unreachable {
		/// Human readable endpoint.
		endpoint: String,
		/// Underlying transport error.
		source: ApiError,
	},
	/// Starting the daemon for this command failed.
	#[error("{0}")]
	DaemonStart(String),
	/// Nothing serves the endpoint.
	#[error("daemon not running at {endpoint}; try `relay daemon start`")]
	NotRunning {
		/// Human readable endpoint.
		endpoint: String,
	},
	/// An RPC failed after connecting.
	#[error("{0}")]
	Api(#[from] ApiError),
	/// The engine finished the operation as failed.
	#[error("{code}: {message}")]
	Rejected {
		/// Stable machine code from the engine.
		code: String,
		/// Sanitized description.
		message: String,
		/// Whether trying again may succeed.
		retryable: bool,
	},
	/// Target resolution failed.
	#[error(transparent)]
	Target(#[from] TargetError),
	/// The request is refused locally, before talking to the engine.
	#[error("{0}")]
	Usage(String),
	/// The engine API cannot do this yet.
	#[error("{0}")]
	Unsupported(String),
	/// Writing output failed.
	#[error("output error: {0}")]
	Io(#[from] std::io::Error),
	/// Encoding JSON output failed.
	#[error("json error: {0}")]
	Json(#[from] serde_json::Error),
	/// A local check or the hosted engine failed.
	#[error("{0:#}")]
	Other(anyhow::Error),
}

impl CliError {
	/// Build from an engine `Problem`.
	pub fn from_problem(problem: &Problem) -> Self {
		Self::Rejected {
			code: problem.code.clone(),
			message: problem.message.clone(),
			retryable: problem.retryable,
		}
	}

	/// Process exit code for this error.
	pub fn exit_code(&self) -> u8 {
		match self {
			Self::Unreachable { .. } | Self::NotRunning { .. } | Self::DaemonStart(_) => {
				exit::UNREACHABLE
			}
			Self::Api(ApiError::OperationTimeout) => exit::TIMEOUT,
			Self::Api(ApiError::Status(status)) => match status.code() {
				tonic::Code::Unavailable => exit::UNREACHABLE,
				tonic::Code::DeadlineExceeded => exit::TIMEOUT,
				tonic::Code::NotFound => exit::TARGET,
				_ => exit::REJECTED,
			},
			Self::Api(_) => exit::FAILURE,
			Self::Rejected { .. } | Self::Unsupported(_) => exit::REJECTED,
			Self::Target(_) => exit::TARGET,
			Self::Usage(_) => exit::USAGE,
			Self::Io(_) | Self::Json(_) | Self::Other(_) => exit::FAILURE,
		}
	}

	/// Stable short code used in `--json` error bodies.
	pub fn code(&self) -> &str {
		match self {
			Self::Unreachable { .. } | Self::NotRunning { .. } | Self::DaemonStart(_) => {
				"engine_unreachable"
			}
			Self::Api(ApiError::OperationTimeout) => "timeout",
			Self::Api(_) => "api_error",
			Self::Rejected { code, .. } => code,
			Self::Target(_) => "target",
			Self::Usage(_) => "usage",
			Self::Unsupported(_) => "unsupported",
			Self::Io(_) => "io",
			Self::Json(_) => "json",
			Self::Other(_) => "failure",
		}
	}
}
