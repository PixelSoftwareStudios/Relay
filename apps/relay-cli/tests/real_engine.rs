//! The CLI against the real engine service (domain core, API layer, daemon
//! host) with recording adapters instead of audio and network hardware.

#![cfg(unix)]

use clap::Parser;
use relay_config::Paths;
use relay_config::identity::MemoryStore;
use relay_core::DeviceKeyId;
use relay_engine::daemon::{DaemonHandle, RunOptions, Transport};
use relay_engine::service::fakes::FakePorts;
use relay_engine::service::{PeerInfo, PeerTrust};

const KITCHEN: u8 = 0x4b;

struct Real {
	daemon: DaemonHandle,
	socket: std::path::PathBuf,
	_dir: tempfile::TempDir,
}

impl Real {
	async fn start() -> Self {
		let dir = tempfile::Builder::new()
			.prefix("rlycli")
			.tempdir_in("/tmp")
			.expect("tempdir");
		let socket = dir.path().join("e.sock");
		let fakes = FakePorts::default();
		fakes.peers.set_known(vec![PeerInfo {
			id: DeviceKeyId::from_bytes([KITCHEN; 32]),
			display_name: "Kitchen".to_owned(),
			addresses: vec!["192.168.1.20:7000".to_owned()],
			trust: PeerTrust::TofuUnverified,
			reachable: true,
			avatar: None,
		}]);
		let ports = fakes.ports();
		let mut options = RunOptions::new(Paths::under(dir.path().join("cfg")));
		options.transport = Transport::Unix(Some(socket.clone()));
		options.identity_store = Some(Box::new(MemoryStore::default()));
		options.adapters = Some(Box::new(move |_| ports));
		let daemon = DaemonHandle::start(options).await.expect("engine starts");
		Self {
			daemon,
			socket,
			_dir: dir,
		}
	}

	async fn run(&self, args: &[&str]) -> Result<String, relay_cli::cli::CliError> {
		let socket = self.socket.to_str().expect("utf8 path");
		let mut argv = vec!["relay", "--socket", socket];
		argv.extend_from_slice(args);
		let cli = relay_cli::cli::Cli::try_parse_from(argv).expect("valid arguments");
		let mut buffer = Vec::new();
		relay_cli::commands::run(cli, &mut buffer).await?;
		Ok(String::from_utf8(buffer).expect("utf8 output"))
	}
}

fn call_prefix(status: &serde_json::Value) -> String {
	status["calls"][0]["id"].as_str().unwrap()[..8].to_owned()
}

#[tokio::test]
async fn status_and_peers_come_from_the_live_engine() {
	let engine = Real::start().await;
	let status: serde_json::Value =
		serde_json::from_str(&engine.run(&["--json", "status"]).await.unwrap()).unwrap();
	assert!(status["calls"].as_array().unwrap().is_empty());
	let peers: serde_json::Value =
		serde_json::from_str(&engine.run(&["--json", "peers"]).await.unwrap()).unwrap();
	assert_eq!(peers[0]["name"], "Kitchen");
	engine.daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_call_by_name_is_listed_and_can_be_ended() {
	let engine = Real::start().await;
	let out = engine.run(&["call", "kitchen"]).await.unwrap();
	assert!(out.contains("applied"), "{out}");
	let status: serde_json::Value =
		serde_json::from_str(&engine.run(&["--json", "status"]).await.unwrap()).unwrap();
	assert_eq!(status["calls"].as_array().unwrap().len(), 1);

	// Every `relay` run is a new client, and the one that started the call still
	// holds its lease: ending it from another run is an explicit takeover.
	assert!(engine.run(&["end", &call_prefix(&status)]).await.is_err());
	let call = call_prefix(&status);
	let out = engine.run(&["--takeover", "end", &call]).await.unwrap();
	assert!(out.contains("applied"), "{out}");
	engine.daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn talk_needs_consent_and_a_mute_applies_engine_wide() {
	let engine = Real::start().await;
	engine.run(&["call", "kitchen"]).await.unwrap();
	let refused = engine.run(&["talk", "on"]).await;
	assert!(refused.is_err(), "talk without --consent must be refused");
	engine.run(&["mute", "on"]).await.unwrap();
	let status: serde_json::Value =
		serde_json::from_str(&engine.run(&["--json", "status"]).await.unwrap()).unwrap();
	assert_eq!(status["muted"], true, "{status}");
	engine.daemon.shutdown().await.unwrap();
}

fn live_global(engine: &Real) -> relay_cli::cli::GlobalArgs {
	relay_cli::cli::Cli::try_parse_from([
		"relay",
		"--socket",
		engine.socket.to_str().unwrap(),
		"status",
	])
	.unwrap()
	.global
}

#[tokio::test]
async fn config_set_goes_through_the_running_engine_not_the_settings_file() {
	let engine = Real::start().await;
	let out = engine
		.run(&["config", "set", "profile.display_name", "Hallway"])
		.await
		.unwrap();
	assert!(out.contains("applied"), "{out}");
	let refused = engine
		.run(&["config", "set", "diagnostics.log_level", "debug"])
		.await;
	assert!(
		refused.is_err(),
		"settings the engine cannot change live are refused"
	);
	engine.daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn controller_lease_comes_from_the_real_engine() {
	use relay_cli::client::{Control, connect};
	let engine = Real::start().await;
	engine.run(&["call", "kitchen"]).await.unwrap();
	let mut client = connect(&live_global(&engine)).await.unwrap();
	let snapshot = client.snapshot().await.unwrap();
	let call = snapshot.calls[0].call_id.clone();
	// Another client already holds the lease, so a plain request is refused.
	assert!(client.controller_generation(&call).await.is_err());
	engine.daemon.shutdown().await.unwrap();
}
