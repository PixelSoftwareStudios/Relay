#![cfg(unix)]
//! Overlay (100.64.0.0/10) addresses: only typed by the user, only when enabled.

mod service_support;
mod support_daemon;

use std::net::SocketAddr;

use relay_api::RelayClient;
use relay_api::pb::{StartCallRequest, start_call_request::Target};
use relay_config::Paths;
use relay_config::prefs::PreferenceStore;
use relay_engine::daemon::DaemonHandle;
use relay_engine::peers::{PeerConfig, SharedTrust, spawn};
use relay_net::discovery::DiscoveredPeer;
use relay_net::identity::Identity;
use service_support::*;
use support_daemon::engine_options;
use tonic::Code;

const OVERLAY: &str = "100.64.1.2:7000";

fn connection_string() -> String {
	format!("relay://{OVERLAY}#{}", "ab".repeat(32))
}

async fn call(client: &mut RelayClient, op: u8, target: Target) -> Result<(), tonic::Status> {
	let request = StartCallRequest {
		context: Some(ctx(client, op)),
		target: Some(target),
		allow_parallel_same_peer: false,
	};
	client.raw().start_call(request).await.map(|_| ())
}

async fn engine(home: &std::path::Path) -> (DaemonHandle, support_daemon::Sim, RelayClient) {
	let (options, sim) = engine_options(home, 440.0);
	let daemon = DaemonHandle::start(options).await.expect("start");
	let mut client = RelayClient::connect(daemon.endpoint())
		.await
		.expect("connect");
	client
		.register("relay-cli", "test", &[])
		.await
		.expect("register");
	(daemon, sim, client)
}

#[tokio::test]
async fn overlay_targets_need_the_setting_and_a_typed_address() {
	let dir = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("dir");

	// Default: refused, with the way to enable it spelled out.
	let (daemon, _sim, mut cli) = engine(dir.path()).await;
	for (op, target) in [
		(1, Target::ManualLocalAddress(OVERLAY.to_owned())),
		(2, Target::ManualConnectionString(connection_string())),
	] {
		let error = call(&mut cli, op, target)
			.await
			.expect_err("refused by default");
		assert_eq!(error.code(), Code::InvalidArgument);
		assert!(
			error.message().contains("manual overlay entry"),
			"{}",
			error.message()
		);
	}
	assert!(cli.snapshot().await.expect("snapshot").calls.is_empty());
	drop(cli);
	daemon.shutdown().await.expect("shutdown");

	// The user opts in.
	{
		let storage =
			relay_config::Storage::open(&Paths::under(dir.path().join("cfg"))).expect("storage");
		PreferenceStore::open(&storage.database)
			.expect("prefs")
			.set("network.allow_overlay_manual", "true")
			.expect("set");
	}
	let (daemon, _sim, mut cli) = engine(dir.path()).await;

	// A pasted connection string names the peer and starts the call.
	call(
		&mut cli,
		3,
		Target::ManualConnectionString(connection_string()),
	)
	.await
	.expect("accepted once enabled");
	assert_eq!(cli.snapshot().await.expect("snapshot").calls.len(), 1);
	// A bare address passes the policy but the peer is unknown: it is dialled first.
	let error = call(&mut cli, 4, Target::ManualLocalAddress(OVERLAY.to_owned()))
		.await
		.expect_err("unknown peer");
	assert_eq!(error.code(), Code::FailedPrecondition);
	daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn a_discovered_overlay_address_is_never_kept() {
	let mut config = PeerConfig::new("local");
	config.listen = "127.0.0.1:0".parse().expect("addr");
	config.discovery = None;
	// Typed overlay entry is on; discovery must not inherit it.
	config.allow_overlay_manual = true;
	let identity = std::sync::Arc::new(Identity::generate().expect("identity"));
	let (peers, _output) = spawn(config, identity, SharedTrust::in_memory(), [1; 16])
		.await
		.expect("spawn");
	let lan: SocketAddr = "192.168.1.5:7000".parse().expect("addr");
	let overlay: SocketAddr = OVERLAY.parse().expect("addr");
	peers
		.note_discovered(DiscoveredPeer {
			fullname: "x._relay._tcp.local.".to_owned(),
			instance_id: [7; 16],
			display_name: "far".to_owned(),
			fingerprint_hint: None,
			capabilities: String::new(),
			addresses: vec![lan, overlay],
			txt: Default::default(),
		})
		.expect("note");
	let listed = loop {
		let snapshot = peers.peers();
		if let Some(entry) = snapshot.iter().find(|p| p.name == "far") {
			break entry.addresses.clone();
		}
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;
	};
	assert_eq!(listed, vec![lan]);
	peers.shutdown();
}
