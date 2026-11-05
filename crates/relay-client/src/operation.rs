//! Waiting for an accepted operation to leave `ACCEPTED`.

use std::future::Future;
use std::time::Duration;

use relay_api::pb::{Operation, OperationState};
use relay_api::{ApiError, RelayClient};
use tokio::sync::Mutex;

/// Anything that can look an operation up by id. Implemented for a client and
/// for a shared client behind a lock (taken per poll, so slow operations
/// never block other commands); tests supply fakes.
pub trait OperationSource {
	/// `GetOperation` by id.
	fn get_operation(
		&mut self,
		operation_id: &[u8],
	) -> impl Future<Output = Result<Operation, ApiError>>;
}

impl OperationSource for RelayClient {
	fn get_operation(
		&mut self,
		operation_id: &[u8],
	) -> impl Future<Output = Result<Operation, ApiError>> {
		RelayClient::get_operation(self, operation_id)
	}
}

impl OperationSource for &Mutex<RelayClient> {
	async fn get_operation(&mut self, operation_id: &[u8]) -> Result<Operation, ApiError> {
		self.lock().await.get_operation(operation_id).await
	}
}

/// Poll `source` until the operation settles. An already settled operation is
/// returned without a call.
pub async fn settle<S: OperationSource>(
	source: &mut S,
	operation: Operation,
	interval: Duration,
	timeout: Duration,
) -> Result<Operation, ApiError> {
	if operation.state != OperationState::Accepted as i32 {
		return Ok(operation);
	}
	let deadline = tokio::time::Instant::now() + timeout;
	loop {
		let op = source.get_operation(&operation.operation_id).await?;
		if op.state != OperationState::Accepted as i32 {
			return Ok(op);
		}
		if tokio::time::Instant::now() + interval > deadline {
			return Err(ApiError::OperationTimeout);
		}
		tokio::time::sleep(interval).await;
	}
}
