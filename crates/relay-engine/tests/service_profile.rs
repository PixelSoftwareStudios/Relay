#![cfg(unix)]
//! Avatar download, connection info and privacy settings on a real engine.

#[macro_use]
mod service_support;
mod support_daemon;

use std::sync::Arc;

use relay_api::RelayClient;
use relay_api::pb::{
	AdvertMode, GetAvatarRequest, InboundPolicy, SetPrivacyRequest, UploadAvatarRequest,
};
use relay_engine::daemon::DaemonHandle;
use relay_net::connect::LocalAddress;
use service_support::*;
use support_daemon::{Sim, engine_options, png};
use tonic::Code;

async fn engine(home: &std::path::Path) -> (DaemonHandle, Sim, RelayClient) {
	let (options, sim) = engine_options(home, 440.0);
	started(options, sim).await
}

async fn started(
	options: relay_engine::daemon::RunOptions,
	sim: Sim,
) -> (DaemonHandle, Sim, RelayClient) {
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

fn home() -> tempfile::TempDir {
	tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("home")
}

#[tokio::test]
async fn an_uploaded_avatar_can_be_fetched_by_its_hash() {
	let dir = home();
	let (daemon, _sim, mut cli) = engine(dir.path()).await;
	let request = UploadAvatarRequest {
		context: Some(ctx(&cli, 1)),
		encoded_image: png(300, 200),
		declared_media_type: "image/png".to_owned(),
	};
	assert_applied(
		&cli.raw()
			.upload_avatar(request)
			.await
			.expect("upload")
			.into_inner(),
	);
	let hash = cli
		.snapshot()
		.await
		.expect("snapshot")
		.settings
		.expect("settings")
		.avatar_sha256;
	assert_eq!(hash.len(), 32);

	let fetch = |hash: Vec<u8>, cli: &RelayClient| GetAvatarRequest {
		client: cli.context(),
		avatar_sha256: hash,
	};
	let request = fetch(hash.clone(), &cli);
	let image = cli
		.raw()
		.get_avatar(request)
		.await
		.expect("fetch")
		.into_inner();
	assert_eq!(image.avatar_sha256, hash);
	assert_eq!((image.width, image.height), (256, 256), "canonical size");
	assert_eq!(image.media_type, "image/webp");
	assert!(!image.image.is_empty() && image.image.len() <= 1024 * 1024);
	assert_eq!(blake3::hash(&image.image).as_bytes().as_slice(), hash);

	let request = fetch(vec![9; 32], &cli);
	let unknown = cli.raw().get_avatar(request).await.expect_err("unknown");
	assert_eq!(unknown.code(), Code::NotFound);
	let request = fetch(vec![1; 5], &cli);
	let malformed = cli.raw().get_avatar(request).await.expect_err("malformed");
	assert_eq!(malformed.code(), Code::InvalidArgument);

	// Reading needs the observe capability, like any other read.
	let mut blind = RelayClient::connect(daemon.endpoint())
		.await
		.expect("connect");
	blind
		.register("blind", "test", &["nothing"])
		.await
		.expect("register");
	let request = fetch(hash, &blind);
	let denied = blind.raw().get_avatar(request).await.expect_err("denied");
	assert_eq!(denied.code(), Code::PermissionDenied);
	daemon.shutdown().await.expect("shutdown");
}

fn interfaces() -> Vec<LocalAddress> {
	let at = |interface: &str, ip: &str| LocalAddress {
		interface: interface.to_owned(),
		ip: ip.parse().expect("ip"),
	};
	vec![
		at("utun3", "10.8.0.2"),
		at("tailscale0", "100.64.1.1"),
		at("en1", "8.8.8.8"),
		at("docker0", "172.17.0.1"),
		at("lo0", "127.0.0.1"),
		at("en0", "fe80::1"),
		at("en2", "169.254.7.7"),
		at("en0", "fd00::5"),
		at("en0", "192.168.1.5"),
	]
}

async fn connection_info(overlay: bool) -> (relay_api::pb::ConnectionInfo, DaemonHandle) {
	let dir = home();
	let (mut options, sim) = engine_options(dir.path(), 440.0);
	options.network.interfaces = Some(Arc::new(interfaces));
	if overlay {
		let storage = relay_config::Storage::open(&options.paths).expect("storage");
		relay_config::prefs::PreferenceStore::open(&storage.database)
			.expect("prefs")
			.set("network.allow_overlay_manual", "true")
			.expect("set");
	}
	let (daemon, _sim, mut cli) = started(options, sim).await;
	let request = cli.context().expect("context");
	let info = cli
		.raw()
		.get_connection_info(request)
		.await
		.expect("info")
		.into_inner();
	std::mem::forget(dir);
	(info, daemon)
}

#[tokio::test]
async fn connection_info_lists_only_eligible_lan_addresses_best_first() {
	for overlay in [false, true] {
		let (info, daemon) = connection_info(overlay).await;
		// Physical private IPv4 first, then ULA IPv6, then IPv4 link-local. No
		// tunnel, public, container, loopback, link-local IPv6 or 100.64.x.x,
		// whether or not typed overlay entry is enabled.
		assert_eq!(
			info.local_addresses,
			["192.168.1.5", "fd00::5", "169.254.7.7"]
		);
		assert_eq!(
			info.listen_port,
			u32::from(daemon.listen_addr().expect("listens").port())
		);
		assert_eq!(info.fingerprint, daemon.local_peer().as_bytes().to_vec());
		assert_eq!(
			info.connection_string,
			format!(
				"relay://192.168.1.5:{}#{}",
				info.listen_port,
				hex::encode(&info.fingerprint)
			)
		);
		daemon.shutdown().await.expect("shutdown");
	}
}

#[tokio::test]
async fn privacy_choices_apply_persist_and_reach_the_snapshot() {
	let dir = home();
	let (daemon, _sim, mut cli) = engine(dir.path()).await;
	let settings = cli
		.snapshot()
		.await
		.expect("snapshot")
		.settings
		.expect("settings");
	assert_eq!(settings.inbound_policy, InboundPolicy::Anyone as i32);
	assert_eq!(settings.advert_mode, AdvertMode::Visible as i32);

	let (_, mut watch) = cli.snapshot_and_watch().await.expect("watch");
	let request = SetPrivacyRequest {
		context: Some(ctx(&cli, 1)),
		inbound_policy: Some(InboundPolicy::SharedChannelOnly as i32),
		advert_mode: Some(AdvertMode::Hidden as i32),
	};
	assert_applied(
		&cli.raw()
			.set_privacy(request)
			.await
			.expect("privacy")
			.into_inner(),
	);
	let relay_api::WatchItem::Event { event, .. } =
		watch.next().await.expect("item").expect("open")
	else {
		panic!("settings event expected");
	};
	let Some(relay_api::pb::state_event::Event::Settings(pushed)) = event.event else {
		panic!("settings event expected");
	};
	assert_eq!(pushed.advert_mode, AdvertMode::Hidden as i32);

	// A request that changes nothing, or names no valid value, is refused.
	for (op, inbound, advert) in [(2, None, None), (3, Some(0), None), (4, None, Some(77))] {
		let request = SetPrivacyRequest {
			context: Some(ctx(&cli, op)),
			inbound_policy: inbound,
			advert_mode: advert,
		};
		let error = cli.raw().set_privacy(request).await.expect_err("refused");
		assert_eq!(error.code(), Code::InvalidArgument);
	}
	// One field alone leaves the other as it was.
	let request = SetPrivacyRequest {
		context: Some(ctx(&cli, 5)),
		inbound_policy: Some(InboundPolicy::Anyone as i32),
		advert_mode: None,
	};
	cli.raw().set_privacy(request).await.expect("partial");
	drop(cli);
	daemon.shutdown().await.expect("shutdown");

	// A restart keeps the choice.
	let (options, sim) = engine_options(dir.path(), 440.0);
	let (daemon, _sim, mut cli) = started(options, sim).await;
	let settings = cli
		.snapshot()
		.await
		.expect("snapshot")
		.settings
		.expect("settings");
	assert_eq!(settings.inbound_policy, InboundPolicy::Anyone as i32);
	assert_eq!(settings.advert_mode, AdvertMode::Hidden as i32);
	daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn hidden_mode_reaches_the_peer_adapter() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let request = SetPrivacyRequest {
		context: Some(ctx(&cli, 1)),
		inbound_policy: None,
		advert_mode: Some(AdvertMode::Hidden as i32),
	};
	assert_applied(
		&cli.raw()
			.set_privacy(request)
			.await
			.expect("privacy")
			.into_inner(),
	);
	let advertised = rig.fakes.peers.profiles();
	assert!(advertised.last().expect("profile pushed").hidden);
	assert!(!advertised.first().expect("initial profile").hidden);
}
