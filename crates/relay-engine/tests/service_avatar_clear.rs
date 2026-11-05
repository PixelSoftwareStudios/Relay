#![cfg(unix)]
//! `ClearAvatar` on real engines: local state, persistence and propagation to a trusted peer.

#[macro_use]
mod service_support;
mod support_daemon;
mod support_pair;

use relay_api::RelayClient;
use relay_api::pb::{GetAvatarRequest, UploadAvatarRequest};
use relay_engine::daemon::DaemonHandle;
use service_support::*;
use support_daemon::{engine_options, png};
use support_pair::paired;
use tonic::Code;

async fn upload(cli: &mut RelayClient, op: u8) -> Vec<u8> {
	let request = UploadAvatarRequest {
		context: Some(ctx(cli, op)),
		encoded_image: png(300, 200),
		declared_media_type: "image/png".to_owned(),
	};
	assert_applied(&cli.raw().upload_avatar(request).await.unwrap().into_inner());
	local_hash(cli).await
}

async fn local_hash(cli: &mut RelayClient) -> Vec<u8> {
	cli.snapshot()
		.await
		.unwrap()
		.settings
		.unwrap()
		.avatar_sha256
}

async fn clear(cli: &mut RelayClient) {
	assert_applied(&cli.clear_avatar().await.expect("clear"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clearing_removes_the_avatar_here_and_on_the_trusted_peer() {
	let (mut a, mut b, b_on_a, a_on_b) = paired().await;
	let hash = upload(&mut a.cli, 10).await;
	assert_eq!(hash.len(), 32);
	let shown = eventually!(
		support_pair::peer_where(&mut b.cli, |p| p.peer_id == a_on_b.peer_id
			&& p.avatar_sha256 == hash)
		.await
	);
	assert_eq!(shown.avatar_sha256, hash);

	clear(&mut a.cli).await;
	assert!(local_hash(&mut a.cli).await.is_empty());
	eventually!(
		support_pair::peer_where(&mut b.cli, |p| p.peer_id == a_on_b.peer_id
			&& p.avatar_sha256.is_empty())
		.await
	);
	// A's view of B is untouched and A is still trusted there.
	assert!(
		support_pair::peer_where(&mut a.cli, |p| p.peer_id == b_on_a.peer_id && p.reachable)
			.await
			.is_some()
	);
	// Clearing twice is fine.
	clear(&mut a.cli).await;
	a.daemon.shutdown().await.unwrap();
	b.daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_cleared_state_survives_a_restart() {
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.unwrap();
	let start = || async {
		let (options, _sim) = engine_options(home.path(), 440.0);
		let daemon = DaemonHandle::start(options).await.unwrap();
		let mut cli = RelayClient::connect(daemon.endpoint()).await.unwrap();
		cli.register("relay-cli", "test", &[]).await.unwrap();
		(daemon, cli)
	};
	let (daemon, mut cli) = start().await;
	let hash = upload(&mut cli, 1).await;
	daemon.shutdown().await.unwrap();

	let (daemon, mut cli) = start().await;
	assert_eq!(
		local_hash(&mut cli).await,
		hash,
		"an uploaded avatar persists"
	);
	clear(&mut cli).await;
	daemon.shutdown().await.unwrap();

	let (daemon, mut cli) = start().await;
	assert!(
		local_hash(&mut cli).await.is_empty(),
		"a cleared avatar stays cleared"
	);
	let request = GetAvatarRequest {
		client: cli.context(),
		avatar_sha256: hash,
	};
	// The bytes are no longer pinned; they may still be cached until evicted.
	let fetched = cli.raw().get_avatar(request).await;
	assert!(fetched.is_ok() || fetched.unwrap_err().code() == Code::NotFound);
	daemon.shutdown().await.unwrap();
}
