#![cfg(unix)]
//! Call history over the API: paging, clearing and the wire shape of an entry.

mod common;

use common::Harness;
use relay_api::pb::{CallDirection, CallHistoryEntry, CallOutcome, OperationState};

fn entry(id: u64) -> CallHistoryEntry {
	CallHistoryEntry {
		id,
		call_id: vec![id as u8; 16],
		peer_name: format!("peer {id}"),
		peer_id: vec![9; 32],
		peer_fingerprint_short: "AAAA-BBBB".into(),
		direction: CallDirection::Incoming as i32,
		outcome: CallOutcome::Missed as i32,
		started_at_unix_ms: 1_800_000_000_000 + id,
		duration_ms: 0,
	}
}

#[tokio::test]
async fn entries_round_trip_newest_first_with_a_cursor() {
	let h = Harness::start().await;
	*h.engine.settings.history.lock().unwrap() = (1..=5).map(entry).collect();
	let mut c = h.client().await;
	let first = c.list_call_history(2, None).await.unwrap();
	let ids: Vec<_> = first.entries.iter().map(|e| e.id).collect();
	assert_eq!(ids, [5, 4]);
	assert!(first.more);
	assert_eq!(first.entries[0], entry(5));
	let rest = c.list_call_history(0, Some(4)).await.unwrap();
	assert_eq!(
		rest.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
		[3, 2, 1]
	);
	assert!(!rest.more);
}

#[tokio::test]
async fn clearing_is_one_applied_mutation() {
	let h = Harness::start().await;
	*h.engine.settings.history.lock().unwrap() = vec![entry(1)];
	let mut c = h.client().await;
	let op = c.clear_call_history().await.unwrap();
	assert_eq!(op.state, OperationState::Applied as i32);
	assert!(
		c.list_call_history(0, None)
			.await
			.unwrap()
			.entries
			.is_empty()
	);
}
