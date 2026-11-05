#![cfg(unix)]
//! Operation idempotency, retention and polling.

#[macro_use]
mod service_support;

use std::time::Duration;

use relay_api::pb::{
	MutationContext, OperationState, SelectDevicesRequest, SetGlobalControlsRequest,
	SetProfileRequest,
};
use service_support::*;
use tonic::Code;

fn profile(client: &relay_api::RelayClient, op: u8, name: &str) -> SetProfileRequest {
	SetProfileRequest {
		context: Some(ctx(client, op)),
		display_name: name.to_owned(),
	}
}

#[tokio::test]
async fn duplicate_operation_returns_the_stored_outcome_without_reexecuting() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;

	let before = rig.fakes.peers.profiles().len();
	let request = profile(&cli, 1, "Kitchen");
	let first = cli
		.raw()
		.set_profile(request.clone())
		.await
		.expect("first")
		.into_inner();
	let again = cli
		.raw()
		.set_profile(request)
		.await
		.expect("again")
		.into_inner();
	assert_applied(&first);
	assert_eq!(first, again);
	assert_eq!(
		rig.fakes.peers.profiles().len(),
		1,
		"the retry must not re-run"
	);

	// Domain commands are deduplicated too, and a retried StartCall is the same call.
	let a = start_call(&mut cli, 2).await;
	let b = start_call(&mut cli, 2).await;
	assert_eq!(a.call_id, b.call_id);
	assert_eq!(cli.snapshot().await.expect("snapshot").calls.len(), 1);
}

#[tokio::test]
async fn reusing_an_operation_id_for_a_different_request_is_rejected() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let request = profile(&cli, 1, "Kitchen");
	cli.raw().set_profile(request).await.expect("first");

	let request = profile(&cli, 1, "Garage");
	let error = cli.raw().set_profile(request).await.expect_err("conflict");
	assert_eq!(error.code(), Code::AlreadyExists);
	assert_eq!(error.message(), "OPERATION_ID_REUSED");

	// Same for a domain command: a different peer under the same id.
	start_call(&mut cli, 5).await;
	let request = relay_api::pb::StartCallRequest {
		context: Some(ctx(&cli, 5)),
		target: Some(relay_api::pb::start_call_request::Target::PeerId(vec![
			9;
			32
		])),
		allow_parallel_same_peer: false,
	};
	let error = cli.raw().start_call(request).await.expect_err("conflict");
	assert_eq!(error.code(), Code::AlreadyExists);
}

#[tokio::test]
async fn operations_are_scoped_to_the_client_that_issued_them() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let mut gui = rig.full_client("desktop").await;
	let request = profile(&cli, 1, "Kitchen");
	cli.raw().set_profile(request).await.expect("profile");

	let found = cli.get_operation(&[1; 16]).await.expect("owner polls");
	assert_eq!(found.state, OperationState::Applied as i32);
	let other = gui.get_operation(&[1; 16]).await.expect_err("not visible");
	assert_eq!(code_of(&other), Code::NotFound);

	// The same id from another client is an independent operation.
	let request = profile(&gui, 1, "Desk");
	gui.raw().set_profile(request).await.expect("independent");
}

#[tokio::test]
async fn polling_reports_domain_and_adapter_operations() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let started = start_call(&mut cli, 3).await;
	let polled = cli
		.wait_operation(&[3; 16], Duration::from_millis(5), Duration::from_secs(2))
		.await
		.expect("poll");
	assert_eq!(polled.state, OperationState::Applied as i32);
	assert_eq!(polled.call_id, started.call_id);

	// A failed adapter operation is recorded and pollable with its problem.
	let request = SelectDevicesRequest {
		context: Some(ctx(&cli, 4)),
		input_device_id: "ghost".to_owned(),
		output_device_id: String::new(),
	};
	let failed = cli
		.raw()
		.select_devices(request)
		.await
		.expect("rpc")
		.into_inner();
	assert_failed(&failed, "NOT_FOUND");
	let polled = cli.get_operation(&[4; 16]).await.expect("poll");
	assert_failed(&polled, "NOT_FOUND");
}

#[tokio::test]
async fn retention_is_bounded_by_count_and_age() {
	let rig = Rig::with(|options| {
		options.limits.operation_cache_per_client = 3;
		options.limits.operation_retention = Duration::from_millis(250);
	})
	.await;
	let mut cli = rig.full_client("relay-cli").await;
	for op in 1..=4u8 {
		let request = profile(&cli, op, &format!("Name {op}"));
		cli.raw().set_profile(request).await.expect("profile");
	}
	// The oldest record fell out of the count bound; the newest remain.
	assert_eq!(
		code_of(&cli.get_operation(&[1; 16]).await.expect_err("evicted")),
		Code::NotFound
	);
	cli.get_operation(&[4; 16]).await.expect("kept");

	tokio::time::sleep(Duration::from_millis(400)).await;
	let request = profile(&cli, 9, "Later");
	cli.raw()
		.set_profile(request)
		.await
		.expect("expires old records");
	assert_eq!(
		code_of(&cli.get_operation(&[4; 16]).await.expect_err("aged out")),
		Code::NotFound
	);
	cli.get_operation(&[9; 16]).await.expect("fresh");
}

#[tokio::test]
async fn stale_settings_revision_fails_without_changing_anything() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let me = cli.context();
	let change = |op: u8, expected: Option<u64>, muted: bool| SetGlobalControlsRequest {
		context: Some(MutationContext {
			client: me.clone(),
			operation_id: vec![op; 16],
			expected_revision: expected,
		}),
		muted: Some(muted),
		deafened: None,
		microphone_gain: None,
		playback_gain: None,
	};
	let request = change(1, Some(0), true);
	assert_applied(
		&cli.raw()
			.set_global_controls(request)
			.await
			.expect("first")
			.into_inner(),
	);
	// Settings are now at revision 1; a writer still holding 0 loses.
	let request = change(2, Some(0), false);
	let lost = cli
		.raw()
		.set_global_controls(request)
		.await
		.expect("rpc")
		.into_inner();
	assert_failed(&lost, "SETTINGS_CONFLICT");
	let settings = cli
		.snapshot()
		.await
		.expect("snapshot")
		.settings
		.expect("settings");
	assert!(settings.global_muted);
}
