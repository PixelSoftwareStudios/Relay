#![cfg(unix)]
//! Channel, avatar, connection-info and secret-handling contract tests.

mod common;

use common::Harness;
use relay_api::pb::{
	AvatarImage, ChannelPolicy, CreateChannelRequest, CreateInviteResponse, JoinChannelRequest,
	OperationState,
};
use relay_api::{AVATAR_MAX_BYTES, ApiError};
use tonic::Code;

fn code(op: &relay_api::pb::Operation) -> &str {
	op.problem.as_ref().map_or("", |p| p.code.as_str())
}

#[tokio::test]
async fn create_list_leave() {
	let h = Harness::start().await;
	let mut c = h.client().await;
	let op = c
		.create_channel("Dorm 4", ChannelPolicy::default(), "hunter2hunter2")
		.await
		.unwrap();
	assert_eq!(op.state, OperationState::Applied as i32);
	let list = c.list_channels().await.unwrap();
	assert_eq!(list.len(), 1);
	assert_eq!(list[0].name, "Dorm 4");
	assert_eq!(list[0].role, "owner");
	let left = c.leave_channel(&list[0].channel_id).await.unwrap();
	assert_eq!(left.state, OperationState::Applied as i32);
	assert!(c.list_channels().await.unwrap().is_empty());
}

#[tokio::test]
async fn invalid_name_and_limit_use_stable_codes() {
	let h = Harness::start().await;
	let mut c = h.client().await;
	let bad = c
		.create_channel("   ", ChannelPolicy::default(), "")
		.await
		.unwrap();
	assert_eq!(code(&bad), "CHANNEL_NAME_INVALID");
	let long = "x".repeat(49);
	let bad = c
		.create_channel(&long, ChannelPolicy::default(), "")
		.await
		.unwrap();
	assert_eq!(code(&bad), "CHANNEL_NAME_INVALID");
	for i in 0..16 {
		let ok = c
			.create_channel(&format!("c{i}"), ChannelPolicy::default(), "")
			.await
			.unwrap();
		assert_eq!(ok.state, OperationState::Applied as i32);
	}
	let full = c
		.create_channel("one-more", ChannelPolicy::default(), "")
		.await
		.unwrap();
	assert_eq!(code(&full), "CHANNEL_LIMIT");
}

#[tokio::test]
async fn join_failures_are_uniform_then_lock_out() {
	let h = Harness::start().await;
	h.engine
		.channels
		.add_remote(&[1; 16], "Lab", "correct-horse");
	let mut c = h.client().await;
	let unknown = c.join_channel(&[9; 16], "x", "").await.unwrap();
	let wrong = c.join_channel(&[1; 16], "wrong", "").await.unwrap();
	assert_eq!(code(&unknown), "CHANNEL_DENIED");
	assert_eq!(code(&wrong), code(&unknown));
	c.join_channel(&[1; 16], "wrong2", "").await.unwrap();
	c.join_channel(&[1; 16], "wrong3", "").await.unwrap();
	let locked = c.join_channel(&[1; 16], "correct-horse", "").await.unwrap();
	assert_eq!(code(&locked), "CHANNEL_LOCKED_OUT");
}

#[tokio::test]
async fn join_with_password_and_single_use_invite() {
	let h = Harness::start().await;
	h.engine
		.channels
		.add_remote(&[2; 16], "Lab", "correct-horse");
	let mut c = h.client().await;
	let ok = c.join_channel(&[2; 16], "correct-horse", "").await.unwrap();
	assert_eq!(ok.state, OperationState::Applied as i32);
	assert_eq!(c.list_channels().await.unwrap().len(), 1);

	let created = c
		.create_channel(
			"Mine",
			ChannelPolicy {
				invite_only: true,
				..Default::default()
			},
			"",
		)
		.await
		.unwrap();
	let id = created.operation_id.clone();
	let invite = c.create_invite(&id, 3600, 1).await.unwrap();
	assert!(invite.invite.starts_with("RLYI1"));
	let left = c.leave_channel(&id).await.unwrap();
	assert_eq!(left.state, OperationState::Applied as i32);
}

#[tokio::test]
async fn non_owner_cannot_invite() {
	let h = Harness::start().await;
	h.engine.channels.add_remote(&[3; 16], "Lab", "pw-pw-pw-pw");
	let mut c = h.client().await;
	c.join_channel(&[3; 16], "pw-pw-pw-pw", "").await.unwrap();
	match c.create_invite(&[3; 16], 60, 1).await {
		Err(ApiError::Status(s)) => assert_eq!(s.code(), Code::PermissionDenied),
		other => panic!("unexpected {other:?}"),
	}
}

#[tokio::test]
async fn avatar_fetch_and_size_bound() {
	let h = Harness::start().await;
	let mut c = h.client().await;
	h.engine
		.channels
		.put_avatar(&[5; 32], vec![0x89, b'P', b'N', b'G']);
	let img: AvatarImage = c.get_avatar(&[5; 32]).await.unwrap();
	assert_eq!(img.png.len(), 4);
	h.engine
		.channels
		.put_avatar(&[6; 32], vec![0; AVATAR_MAX_BYTES + 1]);
	assert!(matches!(
		c.get_avatar(&[6; 32]).await,
		Err(ApiError::InvalidResponse(_))
	));
	match c.get_avatar(&[7; 32]).await {
		Err(ApiError::Status(s)) => assert_eq!(s.code(), Code::NotFound),
		other => panic!("unexpected {other:?}"),
	}
}

#[tokio::test]
async fn connection_info_and_connection_string_target() {
	let h = Harness::start().await;
	let mut c = h.client().await;
	let info = c.connection_info().await.unwrap();
	assert_eq!(info.listen_port, 47000);
	assert!(!info.connection_string.is_empty());
	let op = c
		.start_call_to_connection_string(&info.connection_string)
		.await
		.unwrap();
	assert_eq!(op.state, OperationState::Applied as i32);
	assert_eq!(
		h.engine
			.channels
			.last_connection_string
			.lock()
			.unwrap()
			.as_deref(),
		Some("RLYN1-fake")
	);
}

#[test]
fn secrets_never_appear_in_debug_output() {
	let create = CreateChannelRequest {
		password: "s3cret-pass".into(),
		name: "n".into(),
		..Default::default()
	};
	let join = JoinChannelRequest {
		password: "s3cret-pass".into(),
		invite: "RLYI1-topsecret".into(),
		..Default::default()
	};
	let resp = CreateInviteResponse {
		invite: "RLYI1-topsecret".into(),
		expires_unix: 1,
	};
	for text in [
		format!("{create:?}"),
		format!("{join:?}"),
		format!("{resp:?}"),
	] {
		assert!(
			!text.contains("s3cret") && !text.contains("topsecret"),
			"{text}"
		);
	}
}
