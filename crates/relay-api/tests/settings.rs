#![cfg(unix)]
//! Avatar removal, trust forgetting and settings: client helpers against the contract fake.

mod common;

use common::Harness;
use relay_api::ApiError;
use relay_api::pb::{OperationState, SettingOutcome};

#[tokio::test]
async fn clearing_the_avatar_is_one_applied_mutation() {
	let h = Harness::start().await;
	let mut c = h.client().await;
	let op = c.clear_avatar().await.unwrap();
	assert_eq!(op.state, OperationState::Applied as i32);
	assert_eq!(op.operation_id.len(), 16);
	assert_eq!(*h.engine.settings.avatar_clears.lock().unwrap(), 1);
}

#[tokio::test]
async fn forgetting_names_a_peer_or_fails_with_a_stable_code() {
	let h = Harness::start().await;
	let mut c = h.client().await;
	let ok = c.forget_peer(None, "kitchen").await.unwrap();
	assert_eq!(ok.state, OperationState::Applied as i32);
	assert_eq!(*h.engine.settings.forgotten.lock().unwrap(), ["kitchen"]);
	let by_id = c.forget_peer(Some(&[3; 32]), "").await.unwrap();
	assert_eq!(by_id.state, OperationState::Applied as i32);
	let twin = c.forget_peer(None, "twin").await.unwrap();
	assert_eq!(twin.state, OperationState::Failed as i32);
	assert_eq!(twin.problem.unwrap().code, "PEER_AMBIGUOUS");
	assert!(matches!(
		c.forget_peer(None, " ").await,
		Err(ApiError::Status(_))
	));
}

#[tokio::test]
async fn settings_answer_per_key_and_export_as_a_map() {
	let h = Harness::start().await;
	let mut c = h.client().await;
	let answer = c
		.set_settings([
			("ptt.release_delay_ms", "300"),
			("restart.network.transfer_port", "9"),
			("bad.key", "1"),
		])
		.await
		.unwrap();
	let outcomes: Vec<_> = answer
		.results
		.iter()
		.map(|r| (r.key.as_str(), r.outcome))
		.collect();
	assert_eq!(
		outcomes,
		[
			("bad.key", SettingOutcome::Rejected as i32),
			("ptt.release_delay_ms", SettingOutcome::AppliedLive as i32),
			(
				"restart.network.transfer_port",
				SettingOutcome::SavedNeedsRestart as i32
			),
		]
	);
	assert_eq!(answer.results[0].reason, "not a setting");
	let all = c.get_settings().await.unwrap();
	assert_eq!(all.len(), 2);
	assert_eq!(all["ptt.release_delay_ms"], "300");
}

#[tokio::test]
async fn a_sensitive_key_is_permission_denied() {
	let h = Harness::start().await;
	let mut c = h.client().await;
	let refused = c
		.set_settings([("identity.key_storage", "insecure_file")])
		.await
		.unwrap_err();
	let ApiError::Status(status) = refused else {
		panic!("{refused:?}");
	};
	assert_eq!(status.code(), tonic::Code::PermissionDenied);
	assert!(c.get_settings().await.unwrap().is_empty());
}
