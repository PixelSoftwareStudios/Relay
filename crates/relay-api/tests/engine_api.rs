#![cfg(unix)]
//! End-to-end contract tests over a real Unix socket.

mod common;

use std::time::Duration;

use common::Harness;
use relay_api::pb::{
	CallState, MutationContext, OperationState, StartCallRequest, start_call_request::Target,
	state_event::Event,
};
use relay_api::{ApiError, RelayClient, WatchItem, WatchOptions};
use tonic::Code;

fn start_call(client: &RelayClient, op: u8, peer: u8) -> StartCallRequest {
	StartCallRequest {
		context: Some(MutationContext {
			client: client.context(),
			operation_id: vec![op; 16],
			expected_revision: None,
		}),
		target: Some(Target::PeerId(vec![peer; 32])),
		allow_parallel_same_peer: false,
	}
}

async fn call(
	client: &mut RelayClient,
	op: u8,
	peer: u8,
) -> Result<tonic::Response<relay_api::pb::Operation>, tonic::Status> {
	let request = start_call(client, op, peer);
	client.raw().start_call(request).await
}

#[tokio::test]
async fn get_info_and_register() {
	let h = Harness::start().await;
	let mut client = h.client().await;
	assert_eq!(client.info().api_major, 1);
	assert!(client.info().capabilities.contains(&"observe".to_owned()));
	let ctx = client.context().expect("registered");
	assert_eq!(ctx.client_id.len(), 16);
	assert_eq!(ctx.engine_incarnation, h.registry.incarnation());
	assert_eq!(h.registry.len(), 1);
	assert_eq!(client.snapshot().await.expect("snapshot").revision, 0);
}

#[tokio::test]
async fn snapshot_requires_registration() {
	let h = Harness::start().await;
	let mut client = RelayClient::connect(&h.endpoint()).await.expect("connect");
	assert!(matches!(
		client.snapshot().await,
		Err(ApiError::NotRegistered)
	));
}

#[tokio::test]
async fn watch_delivers_snapshot_then_contiguous_events() {
	let h = Harness::start().await;
	let mut client = h.client().await;
	h.engine.publish_call(b"before-watch-0000");
	let mut watch = client.watch(WatchOptions::default()).await.expect("watch");
	let Some(WatchItem::Snapshot(snap)) = watch.next().await.expect("first") else {
		panic!("first item must be the snapshot");
	};
	assert_eq!(snap.revision, 1);
	let publisher = h.engine.clone();
	tokio::spawn(async move {
		for i in 0..50u8 {
			publisher.publish_call(&[i; 16]);
			tokio::task::yield_now().await;
		}
	});
	for expected in 2..=51u64 {
		match watch.next().await.expect("item").expect("open") {
			WatchItem::Event { revision, event } => {
				assert_eq!(revision, expected);
				assert!(matches!(event.event, Some(Event::Call(_))));
			}
			other => panic!("unexpected {other:?}"),
		}
	}
}

#[tokio::test]
async fn snapshot_then_watch_loses_nothing_under_concurrent_writes() {
	let h = Harness::start().await;
	let mut client = h.client().await;
	let publisher = h.engine.clone();
	let writer = tokio::spawn(async move {
		for i in 0..100u8 {
			publisher.publish_call(&[i; 16]);
			tokio::time::sleep(Duration::from_micros(200)).await;
		}
	});
	tokio::time::sleep(Duration::from_millis(5)).await;
	let (snapshot, mut watch) = client.snapshot_and_watch().await.expect("resume");
	let mut last = snapshot.revision;
	while last < 100 {
		match watch.next().await.expect("no gap").expect("open") {
			WatchItem::Event { revision, .. } => {
				assert_eq!(revision, last + 1);
				last = revision;
			}
			WatchItem::Resync(_) => panic!("replay window is large enough"),
			WatchItem::Snapshot(_) => panic!("resumed watch sends no snapshot"),
		}
	}
	writer.await.expect("writer");
}

#[tokio::test]
async fn resume_beyond_history_requests_resync() {
	let h = Harness::start().await;
	let mut client = h.client().await;
	h.engine.publish_call(b"0123456789abcdef");
	let mut watch = client
		.watch(WatchOptions {
			after_revision: Some(99),
			..WatchOptions::default()
		})
		.await
		.expect("watch");
	assert!(matches!(
		watch.next().await.expect("item"),
		Some(WatchItem::Resync(r)) if r.current_revision == 1
	));
}

#[tokio::test]
async fn duplicate_operation_returns_previous_outcome() {
	let h = Harness::start().await;
	let mut client = h.client().await;
	let first = call(&mut client, 7, 1).await.expect("first").into_inner();
	let again = call(&mut client, 7, 1).await.expect("retry").into_inner();
	assert_eq!(first, again);
	assert_eq!(first.state, OperationState::Applied as i32);
	// The retry must not have created a second state change.
	assert_eq!(
		client.snapshot().await.expect("snapshot").revision,
		first.revision
	);

	let reused = call(&mut client, 7, 2).await;
	assert_eq!(
		reused.expect_err("different command").code(),
		Code::AlreadyExists
	);
}

#[tokio::test]
async fn wait_operation_and_unknown_operation() {
	let h = Harness::start().await;
	let mut client = h.client().await;
	let op = call(&mut client, 3, 1).await.expect("start").into_inner();
	let done = client
		.wait_operation(
			&op.operation_id,
			Duration::from_millis(10),
			Duration::from_secs(1),
		)
		.await
		.expect("done");
	assert_eq!(done.state, OperationState::Applied as i32);
	match client.get_operation(&[9; 16]).await {
		Err(ApiError::Status(s)) => assert_eq!(s.code(), Code::NotFound),
		other => panic!("unexpected {other:?}"),
	}
	let _ = CallState::Connected;
}

#[tokio::test]
async fn bounded_watch_overflows_instead_of_buffering() {
	let h = Harness::start().await;
	let mut client = h.client().await;
	let watch = client.watch(WatchOptions::default()).await.expect("watch");
	let mut bounded = watch.into_bounded(4);
	for i in 0..40u8 {
		h.engine.publish_call(&[i; 16]);
	}
	tokio::time::sleep(Duration::from_millis(100)).await;
	let mut saw_overflow = false;
	while let Some(item) = bounded.recv().await {
		if matches!(item, Err(ApiError::SubscriptionOverflow)) {
			saw_overflow = true;
		}
	}
	assert!(saw_overflow);
}
