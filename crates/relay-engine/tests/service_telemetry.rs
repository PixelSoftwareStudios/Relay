#![cfg(unix)]
//! Telemetry streams and the local microphone test.

#[macro_use]
mod service_support;
mod support_daemon;
mod support_media;

use std::time::{Duration, Instant};

use relay_api::RelayClient;
use relay_api::pb::{
	SelectDevicesRequest, StartMicTestRequest, TelemetryRequest, TelemetrySnapshot,
};
use relay_core::CallId;
use relay_engine::daemon::{DaemonHandle, RunOptions};
use relay_engine::service::telemetry::DropOldest;
use relay_engine::service::{CallMeter, MediaTelemetry};
use service_support::*;
use support_daemon::{Sim, engine_options};
use support_media::rms;
use tonic::{Code, Streaming};

fn request(cli: &RelayClient, calls: Vec<Vec<u8>>, hz: u32) -> TelemetryRequest {
	TelemetryRequest {
		client: cli.context(),
		call_ids: calls,
		meters_hz: hz,
		stats_hz: 0,
	}
}

#[test]
fn a_full_queue_sheds_its_oldest_entries_and_counts_them() {
	let mut queue = DropOldest::new(3);
	for sample in 0..10 {
		queue.push(sample);
	}
	assert_eq!(queue.len(), 3);
	assert_eq!(queue.dropped(), 7);
	assert_eq!(
		[queue.pop(), queue.pop(), queue.pop(), queue.pop()],
		[Some(7), Some(8), Some(9), None]
	);
	assert_eq!(
		DropOldest::<u8>::new(0).dropped(),
		0,
		"capacity is at least one"
	);
}

#[tokio::test]
async fn the_stream_is_rate_limited_ordered_and_filtered() {
	let rig = Rig::start().await;
	let (kept, other) = (CallId::from_u128(7), CallId::from_u128(8));
	let meter = |call: CallId, speaking: bool| CallMeter {
		call: Some(call),
		speaking,
		receive_dbfs: Some(-30.0),
		packets_received: 12,
		..CallMeter::default()
	};
	rig.fakes.media.set_telemetry(MediaTelemetry {
		microphone_dbfs: Some(-20.0),
		capture_overruns: 3,
		render_underruns: 2,
		mic_test_active: false,
		calls: vec![meter(kept, true), meter(other, false)],
	});
	let mut cli = rig.full_client("relay-cli").await;
	let wanted = vec![kept.to_bytes().to_vec()];
	let req = request(&cli, wanted, 1_000);
	let mut stream = cli
		.raw()
		.watch_telemetry(req)
		.await
		.expect("stream")
		.into_inner();

	let started = Instant::now();
	let mut seen: Vec<TelemetrySnapshot> = Vec::new();
	while started.elapsed() < Duration::from_secs(1) {
		let wait = Duration::from_secs(1).saturating_sub(started.elapsed());
		if let Ok(Ok(Some(sample))) = tokio::time::timeout(wait, stream.message()).await {
			seen.push(sample);
		}
	}
	// 1000 Hz was asked for; the engine samples at most 20 times a second.
	assert!((5..=22).contains(&seen.len()), "{} samples", seen.len());
	assert!(
		seen.windows(2)
			.all(|w| w[1].sample_sequence > w[0].sample_sequence)
	);
	assert!(
		seen.windows(2)
			.all(|w| w[1].monotonic_ms_since_engine_start >= w[0].monotonic_ms_since_engine_start)
	);
	let sample = &seen[0];
	assert_eq!(sample.microphone_dbfs, Some(-20.0));
	assert_eq!((sample.capture_overruns, sample.render_underruns), (3, 2));
	assert_eq!(sample.calls.len(), 1, "only the requested call");
	assert_eq!(sample.calls[0].call_id, kept.to_bytes().to_vec());
	assert!(sample.calls[0].speaking);
	assert_eq!(sample.calls[0].receive_dbfs, Some(-30.0));
	assert_eq!(sample.calls[0].packets_received, 12);
}

#[tokio::test]
async fn telemetry_needs_observe_and_sane_filters() {
	let rig = Rig::start().await;
	let mut blind = rig.client("blind", &["nothing"]).await;
	let req = request(&blind, Vec::new(), 10);
	let denied = blind
		.raw()
		.watch_telemetry(req)
		.await
		.map(|_| ())
		.expect_err("denied");
	assert_eq!(denied.code(), Code::PermissionDenied);

	let mut cli = rig.full_client("relay-cli").await;
	let req = request(&cli, vec![vec![1; 3]], 10);
	let bad = cli
		.raw()
		.watch_telemetry(req)
		.await
		.map(|_| ())
		.expect_err("bad id");
	assert_eq!(bad.code(), Code::InvalidArgument);
	let req = request(&cli, vec![vec![1; 16]; 9], 10);
	let many = cli
		.raw()
		.watch_telemetry(req)
		.await
		.map(|_| ())
		.expect_err("too many");
	assert_eq!(many.code(), Code::InvalidArgument);
}

struct Engine {
	daemon: DaemonHandle,
	sim: Sim,
	cli: RelayClient,
	_home: tempfile::TempDir,
}

async fn engine(tune: impl FnOnce(&mut RunOptions)) -> Engine {
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("home");
	let (mut options, sim) = engine_options(home.path(), 440.0);
	tune(&mut options);
	let daemon = DaemonHandle::start(options).await.expect("start");
	let mut cli = RelayClient::connect(daemon.endpoint())
		.await
		.expect("connect");
	cli.register("relay-cli", "test", &[])
		.await
		.expect("register");
	Engine {
		daemon,
		sim,
		cli,
		_home: home,
	}
}

async fn select_microphone(cli: &mut RelayClient, op: u8) {
	// The fake microphone plays a steady tone; noise suppression (on by default) would take it
	// out as noise, and these tests are about the rest of the media path.
	cli.set_settings([("audio.noise_suppression", "false")])
		.await
		.expect("noise suppression off");
	let devices = eventually!(
		Some(cli.snapshot().await.expect("snapshot").devices).filter(|d| !d.is_empty())
	);
	let mic = devices
		.iter()
		.find(|d| d.direction == relay_api::pb::DeviceDirection::Input as i32)
		.expect("microphone")
		.device_id
		.clone();
	let request = SelectDevicesRequest {
		context: Some(ctx(cli, op)),
		input_device_id: mic,
		output_device_id: String::new(),
	};
	assert_applied(
		&cli.raw()
			.select_devices(request)
			.await
			.expect("select")
			.into_inner(),
	);
}

async fn start_test(cli: &mut RelayClient, op: u8, playback: bool) -> relay_api::pb::Operation {
	let request = StartMicTestRequest {
		context: Some(ctx(cli, op)),
		local_playback: playback,
		confirmed_feedback_warning: playback,
	};
	cli.raw()
		.start_mic_test(request)
		.await
		.expect("rpc")
		.into_inner()
}

async fn stop_test(cli: &mut RelayClient, op: u8) {
	let request = ctx(cli, op);
	assert_applied(
		&cli.raw()
			.stop_mic_test(request)
			.await
			.expect("stop")
			.into_inner(),
	);
}

/// Reads samples until the microphone level satisfies `want`.
async fn mic_level(
	stream: &mut Streaming<TelemetrySnapshot>,
	want: impl Fn(Option<f32>) -> bool,
) -> Option<f32> {
	let deadline = Instant::now() + Duration::from_secs(5);
	loop {
		assert!(Instant::now() < deadline, "microphone level never matched");
		let sample = stream.message().await.expect("stream").expect("open");
		if want(sample.microphone_dbfs) {
			return sample.microphone_dbfs;
		}
	}
}

#[tokio::test]
async fn the_mic_test_meters_the_microphone_and_releases_it_when_stopped() {
	let mut e = engine(|_| {}).await;
	// No selection is needed: the system default microphone is used.
	select_microphone(&mut e.cli, 2).await;
	let req = request(&e.cli, Vec::new(), 20);
	let mut stream = e
		.cli
		.raw()
		.watch_telemetry(req)
		.await
		.expect("stream")
		.into_inner();
	mic_level(&mut stream, |level| level.is_none()).await;

	assert_applied(&start_test(&mut e.cli, 3, false).await);
	// The fake microphone plays a 0.5-amplitude sine: about -9 dBFS.
	let level = mic_level(&mut stream, |level| level.is_some())
		.await
		.expect("level");
	assert!((-12.0..=-6.0).contains(&level), "{level}");
	// A second test is refused while one runs.
	assert_failed(&start_test(&mut e.cli, 4, false).await, "INVALID_REQUEST");

	// Nothing about a test is call state: no call, no gate, nothing audible.
	assert!(e.cli.snapshot().await.expect("snapshot").calls.is_empty());
	assert!(e.daemon.gates().load().is_empty());
	assert!(rms(&e.sim.heard()) < 1e-4, "meter-only mode plays nothing");

	stop_test(&mut e.cli, 5).await;
	mic_level(&mut stream, |level| level.is_none()).await;
	assert_applied(&start_test(&mut e.cli, 6, false).await);
	mic_level(&mut stream, |level| level.is_some()).await;
	stop_test(&mut e.cli, 7).await;
	e.daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn local_playback_is_heard_only_while_the_test_runs() {
	let mut e = engine(|_| {}).await;
	select_microphone(&mut e.cli, 1).await;
	// The feedback warning must be confirmed before the speaker is involved.
	let unconfirmed = StartMicTestRequest {
		context: Some(ctx(&e.cli, 2)),
		local_playback: true,
		confirmed_feedback_warning: false,
	};
	let error = e
		.cli
		.raw()
		.start_mic_test(unconfirmed)
		.await
		.expect_err("warning");
	assert_eq!(error.code(), Code::InvalidArgument);

	assert_applied(&start_test(&mut e.cli, 3, true).await);
	let heard = eventually!({
		let all = e.sim.heard();
		let tail = all[all.len().saturating_sub(4_800)..].to_vec();
		(rms(&tail) > 0.05).then_some(tail)
	});
	assert!(
		support_media::power_at(&heard, 440.0) > 10.0 * support_media::power_at(&heard, 1_700.0)
	);
	stop_test(&mut e.cli, 4).await;
	eventually!({
		let all = e.sim.heard();
		(rms(&all[all.len().saturating_sub(4_800)..]) < 1e-3).then_some(())
	});
	e.daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn a_test_ends_by_itself() {
	let mut e = engine(|o| o.mic_test_max = Duration::from_millis(400)).await;
	select_microphone(&mut e.cli, 1).await;
	let req = request(&e.cli, Vec::new(), 20);
	let mut stream = e
		.cli
		.raw()
		.watch_telemetry(req)
		.await
		.expect("stream")
		.into_inner();
	assert_applied(&start_test(&mut e.cli, 2, false).await);
	mic_level(&mut stream, |level| level.is_some()).await;
	mic_level(&mut stream, |level| level.is_none()).await;
	e.daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn a_vanished_client_releases_the_microphone() {
	let mut e = engine(|o| o.idle_client_ttl = Duration::from_millis(300)).await;
	// The watcher keeps its stream open and so stays registered.
	let mut watcher = RelayClient::connect(e.daemon.endpoint())
		.await
		.expect("connect");
	watcher
		.register("desktop", "test", &[])
		.await
		.expect("register");
	let req = request(&watcher, Vec::new(), 20);
	let mut stream = watcher
		.raw()
		.watch_telemetry(req)
		.await
		.expect("stream")
		.into_inner();

	select_microphone(&mut e.cli, 1).await;
	assert_applied(&start_test(&mut e.cli, 2, false).await);
	mic_level(&mut stream, |level| level.is_some()).await;
	// The starter goes quiet without stopping the test.
	mic_level(&mut stream, |level| level.is_none()).await;
	e.daemon.shutdown().await.expect("shutdown");
}
