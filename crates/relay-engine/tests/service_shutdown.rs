#![cfg(unix)]
//! Stopping the engine through the API: admin only, idempotent, graceful.

#[macro_use]
mod service_support;
mod support_daemon;

use std::time::Duration;

use relay_api::RelayClient;
use relay_engine::daemon::RunOptions;
use service_support::*;
use support_daemon::engine_options;
use tonic::Code;

#[tokio::test]
async fn admin_is_granted_only_when_named() {
	let rig = Rig::start().await;
	let mut plain = RelayClient::connect(&relay_api::Endpoint::Unix(rig.socket.clone()))
		.await
		.expect("connect");
	let session = plain.register("a", "test", &[]).await.expect("register");
	assert_eq!(session.granted_capabilities, ["observe", "call-control"]);

	let mut admin = RelayClient::connect(&relay_api::Endpoint::Unix(rig.socket.clone()))
		.await
		.expect("connect");
	let session = admin
		.register("b", "test", &["admin"])
		.await
		.expect("register");
	assert_eq!(session.granted_capabilities, ["admin"]);
}

#[tokio::test]
async fn only_an_admin_client_can_stop_the_engine() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let error = cli.shutdown().await.expect_err("not admin");
	assert_eq!(code_of(&error), Code::PermissionDenied);
	// Denied means nothing happened.
	let asked = tokio::time::timeout(Duration::from_millis(200), rig.daemon.shutdown_requested());
	assert!(asked.await.is_err(), "no stop was requested");

	let mut admin = rig.client("relayd-ctl", &["admin"]).await;
	let first = admin.shutdown().await.expect("shutdown");
	assert_applied(&first);
	tokio::time::timeout(Duration::from_secs(2), rig.daemon.shutdown_requested())
		.await
		.expect("the request is visible to the host");
	// Asking again is harmless.
	assert_applied(&admin.shutdown().await.expect("again"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn run_stops_gracefully_when_asked_and_removes_the_endpoint() {
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("home");
	let (options, _sim): (RunOptions, _) = engine_options(home.path(), 440.0);
	let socket = home.path().join("e.sock");
	let host = tokio::spawn(relay_engine::daemon::run(options));
	eventually!(socket.exists().then_some(()));

	let endpoint = relay_api::Endpoint::Unix(socket.clone());
	let mut watcher = RelayClient::connect(&endpoint).await.expect("connect");
	watcher
		.register("desktop", "test", &["observe"])
		.await
		.expect("register");
	let (_, mut stream) = watcher.snapshot_and_watch().await.expect("watch");
	let mut admin = RelayClient::connect(&endpoint).await.expect("connect");
	admin
		.register("relayd-ctl", "test", &["admin"])
		.await
		.expect("register");
	assert_applied(&admin.shutdown().await.expect("shutdown"));

	let finished = tokio::time::timeout(Duration::from_secs(10), host)
		.await
		.expect("the host exits")
		.expect("task");
	finished.expect("clean stop");
	assert!(!socket.exists(), "the endpoint is removed");
	assert!(RelayClient::connect(&endpoint).await.is_err());
	// Open streams ended instead of hanging.
	let ended = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;
	assert!(matches!(ended, Ok(Ok(None) | Err(_))));
}
