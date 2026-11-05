#![cfg(unix)]
//! The daemon host: configuration, identity, locking, shutdown, idle clients.

#[macro_use]
mod service_support;

use std::time::Duration;

use relay_api::pb::{
	AcquireControllerRequest, SelectDevicesRequest, SetProfileRequest, StartCallRequest,
	TelemetryRequest, UploadAvatarRequest, start_call_request::Target,
};
use relay_api::{Endpoint, RelayClient, WatchOptions};
use relay_config::Paths;
use relay_config::identity::{InsecureFileStore, MemoryStore};
use relay_config::prefs::{KeyStorage, PreferenceStore};
use relay_engine::daemon::{DaemonError, DaemonHandle, RunOptions};
use service_support::*;
use tonic::Code;

fn options(dir: &std::path::Path, socket: &str) -> RunOptions {
	let mut options = RunOptions::new(Paths::under(dir.join("cfg")));
	options.socket = Some(dir.join(socket));
	options.identity_store = Some(Box::new(MemoryStore::default()));
	options
}

#[tokio::test]
async fn capabilities_owned_by_other_modules_report_unimplemented() {
	let dir = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("dir");
	let daemon = DaemonHandle::start(options(dir.path(), "e.sock"))
		.await
		.expect("start");
	let mut cli = RelayClient::connect(&Endpoint::Unix(daemon.socket_path().to_path_buf()))
		.await
		.expect("connect");
	cli.register("relay-cli", "test", &[])
		.await
		.expect("register");

	let request = SelectDevicesRequest {
		context: Some(ctx(&cli, 1)),
		input_device_id: "mic".to_owned(),
		output_device_id: String::new(),
	};
	let error = cli
		.raw()
		.select_devices(request)
		.await
		.expect_err("unwired");
	assert_eq!(error.code(), Code::Unimplemented);

	let request = StartCallRequest {
		context: Some(ctx(&cli, 2)),
		target: Some(Target::ManualLocalAddress("192.168.1.9:7000".to_owned())),
		allow_parallel_same_peer: false,
	};
	let error = cli.raw().start_call(request).await.expect_err("unwired");
	assert_eq!(error.code(), Code::Unimplemented);

	let request = TelemetryRequest {
		client: cli.context(),
		call_ids: Vec::new(),
		meters_hz: 0,
		stats_hz: 0,
	};
	let error = cli
		.raw()
		.watch_telemetry(request)
		.await
		.map(|_| ())
		.expect_err("telemetry");
	assert_eq!(error.code(), Code::Unimplemented);

	// Nothing was recorded for the refused operations: a retry after wiring works.
	assert_eq!(
		code_of(&cli.get_operation(&[1; 16]).await.expect_err("unrecorded")),
		Code::NotFound
	);
	daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn profile_changes_persist_and_bad_avatars_fail_visibly() {
	let dir = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("dir");
	let daemon = DaemonHandle::start(options(dir.path(), "e.sock"))
		.await
		.expect("start");
	let mut cli = RelayClient::connect(&Endpoint::Unix(daemon.socket_path().to_path_buf()))
		.await
		.expect("connect");
	cli.register("relay-cli", "test", &[])
		.await
		.expect("register");

	let request = SetProfileRequest {
		context: Some(ctx(&cli, 1)),
		display_name: "Hallway".to_owned(),
	};
	assert_applied(
		&cli.raw()
			.set_profile(request)
			.await
			.expect("profile")
			.into_inner(),
	);
	let request = SetProfileRequest {
		context: Some(ctx(&cli, 2)),
		display_name: "  ".to_owned(),
	};
	let error = cli
		.raw()
		.set_profile(request)
		.await
		.expect_err("blank name");
	assert_eq!(error.code(), Code::InvalidArgument);

	let request = UploadAvatarRequest {
		context: Some(ctx(&cli, 3)),
		encoded_image: b"definitely not an image".to_vec(),
		declared_media_type: "image/png".to_owned(),
	};
	let failed = cli
		.raw()
		.upload_avatar(request)
		.await
		.expect("rpc")
		.into_inner();
	assert_failed(&failed, "INVALID_REQUEST");

	daemon.shutdown().await.expect("shutdown");
	let storage = relay_config::Storage::open(&Paths::under(dir.path().join("cfg")))
		.expect("lock released on shutdown");
	let loaded = PreferenceStore::open(&storage.database)
		.expect("open")
		.load()
		.expect("load");
	assert_eq!(loaded.preferences.profile.display_name, "Hallway");
}

#[tokio::test]
async fn identity_survives_restart_and_one_process_owns_the_config() {
	let dir = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("dir");
	let key_file = dir.path().join("identity.key");
	let make = |socket: &str| {
		let mut options = options(dir.path(), socket);
		options.identity_store = Some(Box::new(
			InsecureFileStore::new(&key_file, KeyStorage::InsecureFile).expect("store"),
		));
		options
	};
	let first = DaemonHandle::start(make("a.sock")).await.expect("first");
	let local = first.local_peer();

	// A second engine on the same configuration is refused, not interleaved.
	let Err(error) = DaemonHandle::start(make("b.sock")).await else {
		panic!("second writer must be refused");
	};
	assert!(
		matches!(
			error,
			DaemonError::Config(relay_config::ConfigError::Locked(_))
		),
		"{error}"
	);
	first.shutdown().await.expect("shutdown");

	let second = DaemonHandle::start(make("a.sock")).await.expect("restart");
	assert_eq!(second.local_peer(), local);
	second.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn a_second_engine_cannot_take_over_a_live_socket() {
	let first = Rig::start().await;
	let mut other = options(first.dir.path(), "e.sock");
	other.paths = Paths::under(first.dir.path().join("other"));
	let Err(error) = DaemonHandle::start(other).await else {
		panic!("live socket must not be stolen");
	};
	assert!(
		matches!(
			error,
			DaemonError::Api(relay_api::ApiError::AlreadyRunning(_))
		),
		"{error}"
	);
}

#[tokio::test]
async fn shutdown_ends_open_streams_and_removes_the_socket() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let mut watch = cli.watch(WatchOptions::default()).await.expect("watch");
	assert!(watch.next().await.expect("snapshot").is_some());
	let socket = rig.socket.clone();
	tokio::time::timeout(Duration::from_secs(5), rig.daemon.shutdown())
		.await
		.expect("shutdown does not hang on open streams")
		.expect("clean");
	let ended = tokio::time::timeout(Duration::from_secs(2), watch.next()).await;
	assert!(matches!(ended, Ok(Ok(None) | Err(_))), "stream must end");
	assert!(!socket.exists());
}

#[tokio::test]
async fn idle_clients_are_detached_and_release_their_calls() {
	let rig = Rig::with(|options| options.idle_client_ttl = Duration::from_millis(200)).await;
	let mut keeper = rig.full_client("desktop").await;
	let _stream = keeper
		.watch(WatchOptions::default())
		.await
		.expect("watch keeps it alive");
	let mut idle = rig.full_client("relay-cli").await;
	let call = start_call(&mut idle, 1).await.call_id;
	assert!(
		call_in(&mut keeper, &call)
			.await
			.expect("call")
			.controller
			.is_some()
	);

	// The quiet client is dropped and fails closed: its lease is released.
	eventually!(
		call_in(&mut keeper, &call)
			.await
			.filter(|c| c.controller.is_none())
	);
	let error = idle.snapshot().await.expect_err("client is gone");
	assert_eq!(code_of(&error), Code::Unauthenticated);

	// The watcher with an open stream was never touched and can take the call.
	let request = AcquireControllerRequest {
		context: Some(ctx(&keeper, 5)),
		call_id: call.clone(),
		request_explicit_takeover: false,
	};
	keeper
		.raw()
		.acquire_controller(request)
		.await
		.expect("free to acquire");
}

fn png(width: u32, height: u32) -> Vec<u8> {
	let mut out = std::io::Cursor::new(Vec::new());
	image::RgbaImage::from_pixel(width, height, image::Rgba([200, 30, 30, 255]))
		.write_to(&mut out, image::ImageFormat::Png)
		.expect("encode");
	out.into_inner()
}

#[tokio::test]
async fn name_defaults_to_the_hostname_and_the_avatar_is_normalized() {
	let dir = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("dir");
	let daemon = DaemonHandle::start(options(dir.path(), "e.sock"))
		.await
		.expect("start");
	let mut cli = RelayClient::connect(&Endpoint::Unix(daemon.socket_path().to_path_buf()))
		.await
		.expect("connect");
	cli.register("relay-cli", "test", &[])
		.await
		.expect("register");

	let host = gethostname::gethostname().to_string_lossy().into_owned();
	let settings = cli
		.snapshot()
		.await
		.expect("snapshot")
		.settings
		.expect("settings");
	assert_eq!(
		settings.display_name,
		relay_net::discovery::sanitize_display_name(&host)
	);
	assert!(settings.avatar_sha256.is_empty());

	let request = SetProfileRequest {
		context: Some(ctx(&cli, 1)),
		display_name: "Hall <b>way".to_owned(),
	};
	assert_applied(
		&cli.raw()
			.set_profile(request)
			.await
			.expect("profile")
			.into_inner(),
	);
	// Oversized input is cropped and re-encoded to the canonical 128x128 form.
	let request = UploadAvatarRequest {
		context: Some(ctx(&cli, 2)),
		encoded_image: png(300, 200),
		declared_media_type: "image/png".to_owned(),
	};
	assert_applied(
		&cli.raw()
			.upload_avatar(request)
			.await
			.expect("avatar")
			.into_inner(),
	);
	let settings = cli
		.snapshot()
		.await
		.expect("snapshot")
		.settings
		.expect("settings");
	assert_eq!(settings.display_name, "Hall bway");
	assert_eq!(settings.avatar_sha256.len(), 32);
	daemon.shutdown().await.expect("shutdown");
}
