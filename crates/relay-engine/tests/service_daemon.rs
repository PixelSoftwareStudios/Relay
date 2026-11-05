#![cfg(unix)]
//! The daemon host: configuration, identity, locking, shutdown, idle clients.

#[macro_use]
mod service_support;
mod support_daemon;

use std::time::Duration;

use relay_api::pb::{
	AcquireControllerRequest, SelectDevicesRequest, SetGlobalControlsRequest,
	SetNoiseSuppressionRequest, SetProfileRequest, StartCallRequest, StartMicTestRequest,
	TelemetryRequest, UploadAvatarRequest, start_call_request::Target,
};
use relay_api::{RelayClient, WatchOptions};
use relay_config::Paths;
use relay_config::identity::InsecureFileStore;
use relay_config::prefs::{KeyStorage, PreferenceStore};
use relay_engine::daemon::{DaemonError, DaemonHandle, RunOptions, Transport};
use service_support::*;
use support_daemon::{Sim, engine_options};
use tonic::Code;

fn options(dir: &std::path::Path, socket: &str) -> (RunOptions, Sim) {
	let (mut options, sim) = engine_options(dir, 440.0);
	options.transport = Transport::Unix(Some(dir.join(socket)));
	(options, sim)
}

async fn start(dir: &std::path::Path, socket: &str) -> (DaemonHandle, Sim) {
	let (options, sim) = options(dir, socket);
	(DaemonHandle::start(options).await.expect("start"), sim)
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

#[tokio::test]
async fn adapter_backed_operations_apply_fail_or_report_their_limits() {
	let dir = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("dir");
	let (daemon, _sim) = start(dir.path(), "e.sock").await;
	let mut cli = connect(&daemon).await;

	// The fake hardware is listed; a real id selects, a made-up one fails loudly.
	let devices = eventually!(
		Some(cli.snapshot().await.expect("snapshot").devices).filter(|d| !d.is_empty())
	);
	let mic = devices
		.iter()
		.find(|d| d.direction == relay_api::pb::DeviceDirection::Input as i32)
		.expect("a microphone")
		.device_id
		.clone();
	let me = cli.context();
	let select = |op: u8, input: &str| SelectDevicesRequest {
		context: Some(mctx(me.clone(), op)),
		input_device_id: input.to_owned(),
		output_device_id: String::new(),
	};
	let request = select(1, &mic);
	assert_applied(
		&cli.raw()
			.select_devices(request)
			.await
			.expect("rpc")
			.into_inner(),
	);
	let request = select(2, "in:ghost:nothing");
	assert_failed(
		&cli.raw()
			.select_devices(request)
			.await
			.expect("rpc")
			.into_inner(),
		"NOT_FOUND",
	);
	let settings = cli
		.snapshot()
		.await
		.expect("snapshot")
		.settings
		.expect("settings");
	assert_eq!(settings.input_device_id, mic);

	// Noise suppression: off is a bypass; an unqualified backend is refused.
	let noise = |op: u8, id: &str| SetNoiseSuppressionRequest {
		context: Some(mctx(me.clone(), op)),
		qualified_backend_id: id.to_owned(),
	};
	let request = noise(3, "off");
	assert_applied(
		&cli.raw()
			.set_noise_suppression(request)
			.await
			.expect("rpc")
			.into_inner(),
	);
	let request = noise(4, "rnnoise");
	assert_failed(
		&cli.raw()
			.set_noise_suppression(request)
			.await
			.expect("rpc")
			.into_inner(),
		"UNAVAILABLE",
	);

	// Microphone gain is applied to capture and shown in settings.
	let request = SetGlobalControlsRequest {
		context: Some(ctx(&cli, 5)),
		muted: None,
		deafened: None,
		microphone_gain: Some(0.5),
		playback_gain: None,
		ptt_release_delay_ms: None,
	};
	assert_applied(
		&cli.raw()
			.set_global_controls(request)
			.await
			.expect("rpc")
			.into_inner(),
	);
	let settings = cli
		.snapshot()
		.await
		.expect("snapshot")
		.settings
		.expect("settings");
	assert!((settings.microphone_gain - 0.5).abs() < 1e-3);

	// A manual address nobody answers at: the dial starts, the call waits.
	let request = StartCallRequest {
		context: Some(ctx(&cli, 6)),
		target: Some(Target::ManualLocalAddress("127.0.0.1:9".to_owned())),
		allow_parallel_same_peer: false,
	};
	let error = cli
		.raw()
		.start_call(request)
		.await
		.expect_err("not known yet");
	assert_eq!(error.code(), Code::FailedPrecondition);

	// The microphone test is local and runs on the selected microphone.
	let request = StartMicTestRequest {
		context: Some(ctx(&cli, 7)),
		local_playback: false,
		confirmed_feedback_warning: false,
	};
	assert_applied(
		&cli.raw()
			.start_mic_test(request)
			.await
			.expect("mic test")
			.into_inner(),
	);
	let request = TelemetryRequest {
		client: cli.context(),
		call_ids: Vec::new(),
		meters_hz: 0,
		stats_hz: 0,
	};
	let mut stream = cli
		.raw()
		.watch_telemetry(request)
		.await
		.expect("telemetry")
		.into_inner();
	assert!(stream.message().await.expect("sample").is_some());
	let stop = ctx(&cli, 8);
	assert_applied(
		&cli.raw()
			.stop_mic_test(stop)
			.await
			.expect("stop")
			.into_inner(),
	);
	daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn profile_changes_persist_and_bad_avatars_fail_visibly() {
	let dir = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("dir");
	let (daemon, _sim) = start(dir.path(), "e.sock").await;
	let mut cli = connect(&daemon).await;

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
		let (mut options, sim) = options(dir.path(), socket);
		options.identity_store = Some(Box::new(
			InsecureFileStore::new(&key_file, KeyStorage::InsecureFile).expect("store"),
		));
		(options, sim)
	};
	let (first_options, _sim_a) = make("a.sock");
	let first = DaemonHandle::start(first_options).await.expect("first");
	let local = first.local_peer();

	// A second engine on the same configuration is refused, not interleaved.
	let (second_options, _sim_b) = make("b.sock");
	let Err(error) = DaemonHandle::start(second_options).await else {
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

	let (third_options, _sim_c) = make("a.sock");
	let second = DaemonHandle::start(third_options).await.expect("restart");
	assert_eq!(second.local_peer(), local);
	second.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn a_second_engine_cannot_take_over_a_live_socket() {
	let first = Rig::start().await;
	let (mut other, _sim) = options(first.dir.path(), "e.sock");
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
	let (daemon, _sim) = start(dir.path(), "e.sock").await;
	let mut cli = connect(&daemon).await;

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
