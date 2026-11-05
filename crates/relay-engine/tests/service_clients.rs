#![cfg(unix)]
//! A CLI and a GUI client observe and fight over one engine independently.

#[macro_use]
mod service_support;

use relay_api::WatchItem;
use relay_api::pb::{AcquireControllerRequest, CallState};
use service_support::*;
use tonic::Code;

#[tokio::test]
async fn two_clients_keep_independent_sessions() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let mut gui = rig.full_client("desktop").await;
	let mut viewer = rig.client("viewer", &["observe"]).await;
	assert_ne!(
		cli.context().expect("cli").client_id,
		gui.context().expect("gui").client_id
	);

	let (_, mut watch) = viewer.snapshot_and_watch().await.expect("watch");

	let started = start_call(&mut cli, 1).await;
	assert_applied(&started);
	let call = started.call_id.clone();

	// The GUI sees the CLI's call; the CLI is its controller.
	let seen = call_in(&mut gui, &call).await.expect("gui sees call");
	assert_eq!(
		controller_of(&seen).controller_client_id,
		cli.context().expect("cli").client_id
	);

	// An observer-only client cannot mutate.
	let request = mutation(&viewer, 9, &call, 1);
	let denied = viewer
		.raw()
		.end_call(request)
		.await
		.expect_err("observer must not control");
	assert_eq!(denied.code(), Code::PermissionDenied);

	// The viewer's watch received the new call.
	let mut saw_call = false;
	while let Some(item) = watch.next().await.expect("watch") {
		if let WatchItem::Event { event, .. } = item
			&& matches!(
				event.event,
				Some(relay_api::pb::state_event::Event::Call(_))
			) {
			saw_call = true;
			break;
		}
	}
	assert!(saw_call);
}

#[tokio::test]
async fn takeover_invalidates_the_previous_controller() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let mut gui = rig.full_client("desktop").await;
	let call = start_call(&mut cli, 1).await.call_id;
	let first = controller_of(&call_in(&mut cli, &call).await.expect("call"));

	let acquire = |gui: &relay_api::RelayClient, op, takeover| AcquireControllerRequest {
		context: Some(ctx(gui, op)),
		call_id: call.clone(),
		request_explicit_takeover: takeover,
	};

	// Without an explicit takeover the held lease stays put.
	let request = acquire(&gui, 2, false);
	let refused = gui
		.raw()
		.acquire_controller(request)
		.await
		.expect_err("held");
	assert_eq!(refused.code(), Code::FailedPrecondition);
	assert_eq!(refused.message(), "CONTROLLER_HELD");

	let request = acquire(&gui, 3, true);
	let lease = gui
		.raw()
		.acquire_controller(request)
		.await
		.expect("takeover")
		.into_inner();
	assert!(lease.generation > first.generation);
	assert_eq!(
		lease.controller_client_id,
		gui.context().expect("gui").client_id
	);

	// The old controller's generation is now stale: the operation fails, loudly.
	let request = controls(&cli, 4, &call, first.generation, true, None);
	let stale = cli
		.raw()
		.set_call_controls(request)
		.await
		.expect("rpc")
		.into_inner();
	assert_failed(&stale, "STALE_CONTROLLER");

	// The winner controls the call and the CLI observes the change.
	let request = controls(&gui, 5, &call, lease.generation, true, Some(0.5));
	let ok = gui
		.raw()
		.set_call_controls(request)
		.await
		.expect("rpc")
		.into_inner();
	assert_applied(&ok);
	let seen = call_in(&mut cli, &call).await.expect("call");
	assert!(seen.user_muted);
	assert!((seen.receive_gain - 0.5).abs() < 1e-3);
}

#[tokio::test]
async fn call_state_uses_the_api_names_for_negotiating_and_connected() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let call = start_call(&mut cli, 1).await.call_id;
	let (id, _) = connect_call(&rig, &mut cli, &call).await;
	let seen = call_in(&mut cli, &call).await.expect("call");
	assert_eq!(seen.state, CallState::Connected as i32);
	assert_eq!(call_id_of(&seen.call_id), id);
	// Starting a call never authorizes the microphone.
	let route = seen.transmit.expect("route");
	assert!(!route.consent_granted && !route.effective_transmitting);
	// Peer-visible effects were requested of the fake adapters.
	assert_eq!(rig.fakes.peers.links(), vec![peer_id()]);
}

#[tokio::test]
async fn body_context_must_match_the_transport_header() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let gui = rig.full_client("desktop").await;
	let mut forged = ctx(&gui, 1);
	forged.client = gui.context();
	let request = relay_api::pb::StartCallRequest {
		context: Some(forged),
		target: Some(relay_api::pb::start_call_request::Target::PeerId(vec![
			PEER;
			32
		])),
		allow_parallel_same_peer: false,
	};
	let error = cli.raw().start_call(request).await.expect_err("mismatch");
	assert_eq!(error.code(), Code::InvalidArgument);
	assert!(cli.snapshot().await.expect("snapshot").calls.is_empty());
}
