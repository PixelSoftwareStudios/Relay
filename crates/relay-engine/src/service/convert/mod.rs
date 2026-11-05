//! Boundary conversion between wire messages and validated domain types.

mod problem;
mod view;

use relay_api::pb::{ClientContext, MutationContext};
use relay_core::{CallId, ClientId, ControllerLeaseId, DeviceKeyId, OperationId};

use super::error::ServiceError;

pub use problem::{problem_of, service_problem};
pub use view::{device_to_pb, events_of, peer_to_pb, settings_to_pb, snapshot_to_pb};

fn invalid(what: &str) -> ServiceError {
	ServiceError::InvalidArgument(format!("malformed {what}"))
}

fn id16(bytes: &[u8], what: &str) -> Result<u128, ServiceError> {
	<[u8; 16]>::try_from(bytes)
		.map(u128::from_be_bytes)
		.map_err(|_| invalid(what))
}

/// Parses a 16-byte call id.
pub fn call_id(bytes: &[u8]) -> Result<CallId, ServiceError> {
	id16(bytes, "call id").map(CallId::from_u128)
}

/// Parses a 16-byte client id.
pub fn client_id(bytes: &[u8]) -> Result<ClientId, ServiceError> {
	id16(bytes, "client id").map(ClientId::from_u128)
}

/// Parses a 16-byte operation id.
pub fn operation_id(bytes: &[u8]) -> Result<OperationId, ServiceError> {
	id16(bytes, "operation id").map(OperationId::from_u128)
}

/// Parses a 32-byte peer id.
pub fn peer_id(bytes: &[u8]) -> Result<DeviceKeyId, ServiceError> {
	<[u8; 32]>::try_from(bytes)
		.map(DeviceKeyId::from_bytes)
		.map_err(|_| invalid("peer id"))
}

/// The wire form of a controller lease id (a counter, widened to 16 bytes).
pub fn lease_bytes(lease: ControllerLeaseId) -> Vec<u8> {
	u128::from(lease.get()).to_be_bytes().to_vec()
}

/// Parses the wire form of a lease id; empty means "not supplied".
pub fn lease_from_bytes(bytes: &[u8]) -> Result<Option<ControllerLeaseId>, ServiceError> {
	if bytes.is_empty() {
		return Ok(None);
	}
	let value = id16(bytes, "lease id")?;
	u64::try_from(value)
		.map(|value| Some(ControllerLeaseId::new(value)))
		.map_err(|_| invalid("lease id"))
}

/// Checks that a body context names the caller identified by the transport
/// header, and that it does not claim another engine incarnation.
pub fn check_context(
	header: ClientId,
	body: Option<&ClientContext>,
	incarnation: &[u8; 16],
) -> Result<ClientId, ServiceError> {
	let body = body.ok_or_else(|| invalid("client context"))?;
	if client_id(&body.client_id)? != header {
		return Err(ServiceError::ContextMismatch);
	}
	if !body.engine_incarnation.is_empty() && body.engine_incarnation != incarnation {
		return Err(ServiceError::IncarnationChanged);
	}
	Ok(header)
}

/// A mutation's validated envelope.
#[derive(Debug, Clone, Copy)]
pub struct Mutation {
	/// The caller.
	pub client: ClientId,
	/// Its idempotency key.
	pub operation: OperationId,
	/// Optimistic settings revision, if supplied.
	pub expected_revision: Option<u64>,
}

/// Validates a [`MutationContext`] against the header.
pub fn check_mutation(
	header: ClientId,
	context: Option<&MutationContext>,
	incarnation: &[u8; 16],
) -> Result<Mutation, ServiceError> {
	let context = context.ok_or_else(|| invalid("mutation context"))?;
	let client = check_context(header, context.client.as_ref(), incarnation)?;
	Ok(Mutation {
		client,
		operation: operation_id(&context.operation_id)?,
		expected_revision: context.expected_revision,
	})
}
