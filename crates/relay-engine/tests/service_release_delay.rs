#![cfg(unix)]
//! The push-to-talk release delay as a setting: default, live change through
//! `SetGlobalControls`, persistence across a restart, and range checks.

#[macro_use]
mod service_support;
mod support_daemon;

use relay_api::RelayClient;
use relay_api::pb::{Operation, SetGlobalControlsRequest};
use relay_engine::daemon::{DaemonHandle, Transport};
use service_support::*;
use support_daemon::engine_options;

fn request(client: &RelayClient, op: u8, delay: u32) -> SetGlobalControlsRequest {
	SetGlobalControlsRequest {
		context: Some(ctx(client, op)),
		muted: None,
		deafened: None,
		microphone_gain: None,
		playback_gain: None,
		ptt_release_delay_ms: Some(delay),
	}
}

async fn delay_of(client: &mut RelayClient) -> u32 {
	client
		.snapshot()
		.await
		.expect("snapshot")
		.settings
		.expect("settings")
		.ptt_release_delay_ms
}

async fn connect(daemon: &DaemonHandle) -> RelayClient {
	let mut client = RelayClient::connect(daemon.endpoint())
		.await
		.expect("connect");
	client
		.register("relay-cli", "test", &[])
		.await
		.expect("register");
	client
}

async fn set(client: &mut RelayClient, op: u8, delay: u32) -> Operation {
	let request = request(client, op, delay);
	client
		.raw()
		.set_global_controls(request)
		.await
		.expect("rpc")
		.into_inner()
}

#[tokio::test]
async fn the_delay_defaults_changes_live_persists_and_is_range_checked() {
	let dir = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("dir");
	let start = || async {
		let (mut options, sim) = engine_options(dir.path(), 440.0);
		options.transport = Transport::Unix(Some(dir.path().join("e.sock")));
		(DaemonHandle::start(options).await.expect("start"), sim)
	};

	let (daemon, _sim) = start().await;
	let mut cli = connect(&daemon).await;
	assert_eq!(delay_of(&mut cli).await, 200, "default");

	assert_applied(&set(&mut cli, 1, 750).await);
	assert_eq!(delay_of(&mut cli).await, 750);
	assert_applied(&set(&mut cli, 2, 0).await);
	assert_eq!(delay_of(&mut cli).await, 0);
	assert_applied(&set(&mut cli, 3, 2000).await);

	let too_long = request(&cli, 4, 2001);
	let rejected = cli.raw().set_global_controls(too_long).await;
	assert!(rejected.is_err(), "above the 2000 ms maximum");
	assert_eq!(
		delay_of(&mut cli).await,
		2000,
		"unchanged after a rejection"
	);

	// A restart reads the stored value back into the domain.
	daemon.shutdown().await.expect("shutdown");
	let (daemon, _sim) = start().await;
	let mut cli = connect(&daemon).await;
	assert_eq!(delay_of(&mut cli).await, 2000);
	daemon.shutdown().await.expect("shutdown");
}
