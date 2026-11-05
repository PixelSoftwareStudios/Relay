//! Service-level failures and their mapping to gRPC statuses.

use thiserror::Error;
use tonic::Status;

use super::ports::PortError;

/// Why a service call could not be completed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ServiceError {
	/// A request field was malformed.
	#[error("invalid argument: {0}")]
	InvalidArgument(String),
	/// The body's client context differs from the transport header.
	#[error("client context in body does not match the request header")]
	ContextMismatch,
	/// The caller's engine incarnation is not ours.
	#[error("engine incarnation changed")]
	IncarnationChanged,
	/// The client lacks the capability for this call.
	#[error("capability `{0}` was not granted")]
	CapabilityDenied(&'static str),
	/// The operation id was reused with a different request.
	#[error("operation id reused with a different request")]
	OperationReused,
	/// A bounded queue is full; back off and retry.
	#[error("engine is busy")]
	Busy,
	/// The feature is owned by a module that is not wired in.
	#[error("{0} is not available in this build")]
	Unimplemented(&'static str),
	/// The control actor has stopped.
	#[error("engine is shutting down")]
	Stopped,
	/// A precondition on engine state failed.
	#[error("failed precondition: {0}")]
	Precondition(String),
	/// The client limit was reached.
	#[error("client limit reached")]
	ClientLimit,
	/// Startup of the service failed.
	#[error("invalid configuration: {0}")]
	Config(String),
}

impl From<ServiceError> for Status {
	fn from(error: ServiceError) -> Self {
		let message = error.to_string();
		match error {
			ServiceError::InvalidArgument(_) | ServiceError::ContextMismatch => {
				Status::invalid_argument(message)
			}
			ServiceError::IncarnationChanged => {
				Status::failed_precondition("ENGINE_INCARNATION_CHANGED")
			}
			ServiceError::CapabilityDenied(_) => Status::permission_denied(message),
			ServiceError::OperationReused => Status::already_exists("OPERATION_ID_REUSED"),
			ServiceError::Busy | ServiceError::ClientLimit => Status::resource_exhausted(message),
			ServiceError::Unimplemented(_) => Status::unimplemented(message),
			ServiceError::Stopped => Status::unavailable(message),
			ServiceError::Precondition(_) => Status::failed_precondition(message),
			ServiceError::Config(_) => Status::internal(message),
		}
	}
}

impl From<PortError> for ServiceError {
	fn from(error: PortError) -> Self {
		match error {
			PortError::Unsupported(what) => Self::Unimplemented(what),
			PortError::Invalid(why) => Self::InvalidArgument(why),
			PortError::NotFound(what) => Self::Precondition(format!("not found: {what}")),
			PortError::Unavailable(why) => Self::Precondition(why),
		}
	}
}
