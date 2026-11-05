#![cfg(unix)]
//! Two complete engines in one process: separate homes, real loopback
//! sockets and Noise links, fake audio hardware, driven only through the
//! local API. Pair, call, consent, hear each other, hang up.

#[macro_use]
mod service_support;
mod support_daemon;
mod support_media;

use std::time::Duration;

use relay_api::RelayClient;
use relay_api::pb::{
	AcquireControllerRequest, CallState, DecidePeerTrustRequest, DeviceDirection, Peer,
	SelectDevicesRequest, SetTransmitRouteRequest, StartCallRequest, StartMicTestRequest,
	TelemetryRequest, TransmitMode, TrustState, start_call_request::Target,
};
use relay_engine::daemon::DaemonHandle;
use service_support::*;
use support_daemon::{Sim, engine_options};
use support_media::{power_at, rms};
use tonic::Code;

const TONE_A: f32 = 440.0;
const TONE_B: f32 = 1_700.0;

struct Side {
	daemon: DaemonHandle,
	sim: Sim,
	cli: RelayClient,
	/// An open watch keeps the client attached, so its calls live.
	_watch: relay_api::StateWatch,
	_home: tempfile::TempDir,
}

async fn side(tone: f32) -> Side {
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("home");
	let (options, sim) = engine_options(home.path(), tone);
	let daemon = DaemonHandle::start(options).await.expect("engine starts");
	let mut cli = RelayClient::connect(daemon.endpoint())
		.await
		.expect("connect");
	cli.register("relay-cli", "test", &[])
		.await
		.expect("register");
	let watch = cli
		.watch(relay_api::WatchOptions::default())
		.await
		.expect("watch");
	Side {
		daemon,
		sim,
		cli,
		_watch: watch,
		_home: home,
	}
}

/// The newest `ms` milliseconds the side's speaker played.
fn tail(side: &Side, ms: usize) -> Vec<f32> {
	let heard = side.sim.heard();
	heard[heard.len().saturating_sub(48 * ms)..].to_vec()
}

async fn warned(cli: &mut RelayClient, code: &str) -> bool {
	let snapshot = cli.snapshot().await.expect("snapshot");
	snapshot.warnings.iter().any(|w| w.code == code)
}

async fn pin(cli: &mut RelayClient, op: u8, peer: &Peer) {
	let request = DecidePeerTrustRequest {
		context: Some(ctx(cli, op)),
		peer_id: peer.peer_id.clone(),
		expected_public_key_sha256: peer.peer_id.clone(),
		requested_trust: TrustState::TofuUnverified as i32,
		out_of_band_verified: false,
	};
	assert_applied(
		&cli.raw()
			.decide_peer_trust(request)
			.await
			.expect("pin")
			.into_inner(),
	);
}

async fn select_microphone(cli: &mut RelayClient, op: u8) {
	let devices = eventually!(
		Some(cli.snapshot().await.expect("snapshot").devices).filter(|d| !d.is_empty())
	);
	let mic = devices
		.iter()
		.find(|d| d.direction == DeviceDirection::Input as i32)
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

async fn route(cli: &mut RelayClient, op: u8, call: &[u8], mode: TransmitMode) {
	let generation = controller_of(&call_in(cli, call).await.expect("call")).generation;
	let request = SetTransmitRouteRequest {
		context: Some(ctx(cli, op)),
		call_id: call.to_vec(),
		controller_generation: generation,
		mode: mode as i32,
		explicit_microphone_consent: mode != TransmitMode::ReceiveOnly,
		authorize_background_open_mic: false,
	};
	assert_applied(
		&cli.raw()
			.set_transmit_route(request)
			.await
			.expect("route")
			.into_inner(),
	);
}

/// Two engines with a connected call and nobody consented yet.
async fn connected_pair() -> (Side, Side, Vec<u8>) {
	let mut a = side(TONE_A).await;
	let mut b = side(TONE_B).await;
	let b_addr = b.daemon.listen_addr().expect("b listens");

	// A types B's address. The peer is unknown, so the engine dials and asks to retry.
	let request = StartCallRequest {
		context: Some(ctx(&a.cli, 1)),
		target: Some(Target::ManualLocalAddress(b_addr.to_string())),
		allow_parallel_same_peer: false,
	};
	let refused = a.cli.raw().start_call(request).await.expect_err("dialling");
	assert_eq!(refused.code(), Code::FailedPrecondition);

	// Both sides authenticated an unknown key: nothing proceeds until each user decides.
	eventually!(warned(&mut a.cli, "TRUST_REQUIRED").await.then_some(()));
	eventually!(warned(&mut b.cli, "TRUST_REQUIRED").await.then_some(()));
	let b_on_a = eventually!(
		a.cli
			.snapshot()
			.await
			.expect("snapshot")
			.peers
			.into_iter()
			.find(|p| p.trust == TrustState::Unknown as i32)
	);
	let a_on_b = eventually!(
		b.cli
			.snapshot()
			.await
			.expect("snapshot")
			.peers
			.into_iter()
			.find(|p| p.trust == TrustState::Unknown as i32)
	);
	assert_eq!(b_on_a.peer_id, b.daemon.local_peer().as_bytes().to_vec());
	assert_eq!(a_on_b.peer_id, a.daemon.local_peer().as_bytes().to_vec());
	pin(&mut a.cli, 2, &b_on_a).await;
	pin(&mut b.cli, 3, &a_on_b).await;
	eventually!(
		a.cli
			.snapshot()
			.await
			.expect("snapshot")
			.peers
			.iter()
			.find(|p| p.reachable && p.trust == TrustState::TofuUnverified as i32)
			.map(|_| ())
	);

	// Calling by identity now works; B sees it ring and accepts as the controller.
	let request = StartCallRequest {
		context: Some(ctx(&a.cli, 4)),
		target: Some(Target::PeerId(b_on_a.peer_id.clone())),
		allow_parallel_same_peer: false,
	};
	let started = a
		.cli
		.raw()
		.start_call(request)
		.await
		.expect("call")
		.into_inner();
	let call = started.call_id;
	let ringing = eventually!(
		call_in(&mut b.cli, &call)
			.await
			.filter(|c| c.state == CallState::IncomingRinging as i32)
	);
	assert_eq!(ringing.peer_id, a.daemon.local_peer().as_bytes().to_vec());
	let request = AcquireControllerRequest {
		context: Some(ctx(&b.cli, 5)),
		call_id: call.clone(),
		request_explicit_takeover: false,
	};
	let lease = b
		.cli
		.raw()
		.acquire_controller(request)
		.await
		.expect("lease")
		.into_inner();
	let accept = mutation(&b.cli, 6, &call, lease.generation);
	assert_applied(
		&b.cli
			.raw()
			.accept_call(accept)
			.await
			.expect("accept")
			.into_inner(),
	);
	wait_state(&mut a.cli, &call, CallState::Connected).await;
	wait_state(&mut b.cli, &call, CallState::Connected).await;
	(a, b, call)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_engines_pair_call_and_talk_only_with_consent() {
	let (mut a, mut b, call) = connected_pair().await;

	// Connected and silent: nobody consented, and no microphone is even selected.
	tokio::time::sleep(Duration::from_millis(500)).await;
	assert!(rms(&tail(&a, 300)) < 1e-4 && rms(&tail(&b, 300)) < 1e-4);

	// A consents: B hears A's tone only.
	select_microphone(&mut a.cli, 7).await;
	select_microphone(&mut b.cli, 8).await;
	route(&mut a.cli, 9, &call, TransmitMode::OpenMic).await;
	let at_b = eventually!({
		let heard = tail(&b, 400);
		(rms(&heard) > 0.05).then_some(heard)
	});
	assert!(power_at(&at_b, TONE_A) > 20.0 * power_at(&at_b, TONE_B));
	assert!(rms(&tail(&a, 300)) < 1e-4, "B has not consented");

	// B consents too: A hears B's tone.
	route(&mut b.cli, 10, &call, TransmitMode::OpenMic).await;
	let at_a = eventually!({
		let heard = tail(&a, 400);
		(rms(&heard) > 0.05).then_some(heard)
	});
	assert!(power_at(&at_a, TONE_B) > 20.0 * power_at(&at_a, TONE_A));

	// Withdrawing consent closes A's gate before the answer, and B goes quiet.
	route(&mut a.cli, 11, &call, TransmitMode::ReceiveOnly).await;
	assert!(!a.daemon.gates().load().is_open(call_id_of(&call)));
	tokio::time::sleep(Duration::from_millis(600)).await;
	assert!(
		rms(&tail(&b, 200)) < 1e-3,
		"B hears silence after withdrawal"
	);

	// A hangs up; both sides end the call and close the gates.
	let generation = controller_of(&call_in(&mut a.cli, &call).await.expect("call")).generation;
	let hangup = mutation(&a.cli, 12, &call, generation);
	assert_applied(
		&a.cli
			.raw()
			.end_call(hangup)
			.await
			.expect("hangup")
			.into_inner(),
	);
	wait_state(&mut a.cli, &call, CallState::Ended).await;
	wait_state(&mut b.cli, &call, CallState::Ended).await;
	assert!(!b.daemon.gates().load().is_open(call_id_of(&call)));

	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_microphone_test_during_a_call_never_transmits_and_speech_is_reported() {
	let (mut a, mut b, call) = connected_pair().await;
	select_microphone(&mut a.cli, 20).await;
	let request = StartMicTestRequest {
		context: Some(ctx(&a.cli, 21)),
		local_playback: true,
		confirmed_feedback_warning: true,
	};
	let started = a
		.cli
		.raw()
		.start_mic_test(request)
		.await
		.expect("test")
		.into_inner();
	assert_applied(&started);

	// A hears its own microphone; B hears nothing, and A's gate stays shut.
	eventually!((rms(&tail(&a, 300)) > 0.05).then_some(()));
	tokio::time::sleep(Duration::from_millis(800)).await;
	assert!(
		rms(&tail(&b, 500)) < 1e-4,
		"the test must not reach the peer"
	);
	assert!(!a.daemon.gates().load().is_open(call_id_of(&call)));
	let transmit = call_in(&mut a.cli, &call)
		.await
		.expect("call")
		.transmit
		.expect("route");
	assert!(!transmit.consent_granted && !transmit.effective_transmitting);
	let req = TelemetryRequest {
		client: b.cli.context(),
		call_ids: vec![call.clone()],
		meters_hz: 20,
		stats_hz: 0,
	};
	let mut stream = b
		.cli
		.raw()
		.watch_telemetry(req)
		.await
		.expect("stream")
		.into_inner();
	let quiet = stream.message().await.expect("sample").expect("open");
	assert!(!quiet.calls[0].speaking && quiet.calls[0].receive_dbfs.is_none());

	let stop = ctx(&a.cli, 22);
	assert_applied(
		&a.cli
			.raw()
			.stop_mic_test(stop)
			.await
			.expect("stop")
			.into_inner(),
	);

	// With consent the same microphone does reach B, and B's meters show speech.
	route(&mut a.cli, 23, &call, TransmitMode::OpenMic).await;
	let loud = loop {
		let sample = stream.message().await.expect("sample").expect("open");
		if sample.calls[0].speaking {
			break sample;
		}
	};
	assert!(
		loud.calls[0]
			.receive_dbfs
			.is_some_and(|level| level > -30.0)
	);
	assert!(loud.calls[0].packets_received > 0);
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn telemetry_reports_the_control_link_round_trip_once_a_keepalive_is_echoed() {
	let (a, b, call) = connected_pair().await;
	let req = TelemetryRequest {
		client: a.cli.context(),
		call_ids: vec![call],
		meters_hz: 10,
		stats_hz: 0,
	};
	let mut stream = a
		.cli
		.raw()
		.watch_telemetry(req)
		.await
		.expect("stream")
		.into_inner();
	let first = stream.message().await.expect("sample").expect("open");
	let early = &first.calls[0];
	if early.rtt_ms.is_none() {
		assert_eq!(early.latency_estimate_method, "unmeasured");
	}
	// The first keepalive goes out after one interval (5 s) and is echoed at once.
	let timed = tokio::time::timeout(Duration::from_secs(15), async {
		loop {
			let sample = stream.message().await.expect("sample").expect("open");
			if sample.calls[0].rtt_ms.is_some() {
				break sample;
			}
		}
	})
	.await
	.expect("a keepalive was echoed");
	let call = &timed.calls[0];
	let rtt = call.rtt_ms.expect("rtt");
	assert!(
		rtt > 0.0 && rtt < 250.0,
		"loopback round trip, got {rtt} ms"
	);
	assert_eq!(call.latency_estimate_method, "control_keepalive_rtt");
	assert!(
		call.estimated_mouth_to_ear_ms.is_none(),
		"mouth-to-ear is not derived from a control-link probe"
	);
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}
