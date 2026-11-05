//! Waiting for an accepted operation to leave `ACCEPTED`.

use std::future::Future;
use std::time::Duration;

use relay_api::ApiError;
use relay_api::pb::{Operation, OperationState};

/// Poll `fetch` (a `GetOperation` by id) until the operation settles.
/// An already settled operation is returned without a call. The fetch is a
/// closure so callers decide how they borrow the client between polls.
pub async fn settle<F, Fut>(
	operation: Operation,
	interval: Duration,
	timeout: Duration,
	mut fetch: F,
) -> Result<Operation, ApiError>
where
	F: FnMut(Vec<u8>) -> Fut,
	Fut: Future<Output = Result<Operation, ApiError>>,
{
	if operation.state != OperationState::Accepted as i32 {
		return Ok(operation);
	}
	let deadline = tokio::time::Instant::now() + timeout;
	loop {
		let op = fetch(operation.operation_id.clone()).await?;
		if op.state != OperationState::Accepted as i32 {
			return Ok(op);
		}
		if tokio::time::Instant::now() + interval > deadline {
			return Err(ApiError::OperationTimeout);
		}
		tokio::time::sleep(interval).await;
	}
}
