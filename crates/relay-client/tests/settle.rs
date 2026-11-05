use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use relay_api::ApiError;
use relay_api::pb::{Operation, OperationState};
use relay_client::operation::settle;

fn op(state: OperationState) -> Operation {
	Operation {
		operation_id: vec![9; 16],
		state: state as i32,
		..Operation::default()
	}
}

const POLL: Duration = Duration::from_millis(10);

#[tokio::test(start_paused = true)]
async fn settled_operations_are_returned_without_polling() {
	let calls = AtomicU32::new(0);
	let done = settle(op(OperationState::Applied), POLL, POLL * 5, |_| async {
		calls.fetch_add(1, Ordering::SeqCst);
		Ok(op(OperationState::Applied))
	})
	.await
	.unwrap();
	assert_eq!(done.state, OperationState::Applied as i32);
	assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn accepted_operations_are_polled_until_they_settle() {
	let calls = AtomicU32::new(0);
	let done = settle(op(OperationState::Accepted), POLL, POLL * 50, |id| {
		assert_eq!(id, vec![9; 16]);
		let n = calls.fetch_add(1, Ordering::SeqCst);
		async move {
			Ok(op(if n < 2 {
				OperationState::Accepted
			} else {
				OperationState::Failed
			}))
		}
	})
	.await
	.unwrap();
	assert_eq!(done.state, OperationState::Failed as i32);
	assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test(start_paused = true)]
async fn a_stuck_operation_times_out() {
	let err = settle(op(OperationState::Accepted), POLL, POLL * 3, |_| async {
		Ok(op(OperationState::Accepted))
	})
	.await
	.unwrap_err();
	assert!(matches!(err, ApiError::OperationTimeout), "{err}");
}
