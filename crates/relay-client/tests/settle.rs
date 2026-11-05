use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use relay_api::ApiError;
use relay_api::pb::{Operation, OperationState};
use relay_client::operation::{OperationSource, settle};

fn op(state: OperationState) -> Operation {
	Operation {
		operation_id: vec![9; 16],
		state: state as i32,
		..Operation::default()
	}
}

struct Fake<F>(F);

impl<F: FnMut(&[u8]) -> Operation> OperationSource for Fake<F> {
	async fn get_operation(&mut self, id: &[u8]) -> Result<Operation, ApiError> {
		Ok((self.0)(id))
	}
}

const POLL: Duration = Duration::from_millis(10);

#[tokio::test(start_paused = true)]
async fn settled_operations_are_returned_without_polling() {
	let calls = AtomicU32::new(0);
	let mut source = Fake(|_: &[u8]| {
		calls.fetch_add(1, Ordering::SeqCst);
		op(OperationState::Applied)
	});
	let done = settle(&mut source, op(OperationState::Applied), POLL, POLL * 5)
		.await
		.unwrap();
	assert_eq!(done.state, OperationState::Applied as i32);
	assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn accepted_operations_are_polled_until_they_settle() {
	let calls = AtomicU32::new(0);
	let mut source = Fake(|id: &[u8]| {
		assert_eq!(id, [9; 16]);
		let n = calls.fetch_add(1, Ordering::SeqCst);
		op(if n < 2 {
			OperationState::Accepted
		} else {
			OperationState::Failed
		})
	});
	let done = settle(&mut source, op(OperationState::Accepted), POLL, POLL * 50)
		.await
		.unwrap();
	assert_eq!(done.state, OperationState::Failed as i32);
	assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test(start_paused = true)]
async fn a_stuck_operation_times_out() {
	let mut source = Fake(|_: &[u8]| op(OperationState::Accepted));
	let err = settle(&mut source, op(OperationState::Accepted), POLL, POLL * 3)
		.await
		.unwrap_err();
	assert!(matches!(err, ApiError::OperationTimeout), "{err}");
}
