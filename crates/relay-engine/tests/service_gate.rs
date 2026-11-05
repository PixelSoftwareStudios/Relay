#![cfg(unix)]
//! The published transmit gate follows consent, push-to-talk leases and
//! safety denies, and is closed before anything is acknowledged.

#[macro_use]
mod service_support;

use relay_api::RelayClient;
use relay_api::pb::{
	AcquireControllerRequest, InputAction, InputControlAck, InputControlMessage,
	SetGlobalControlsRequest, SetTransmitRouteRequest, TransmitMode,
};
use relay_core::{CallId, SystemEvent};
use relay_engine::service::fakes::MediaCall;
use service_support::*;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Streaming};

struct Talk {
	cli: RelayClient,
	call: Vec<u8>,
	id: CallId,
	generation: u64,
}

async fn connected(rig: &Rig) -> Talk {
	let mut cli = rig.full_client("relay-cli").await;
	let call = start_call(&mut cli, 1).await.call_id;
	let (id, _) = connect_call(rig, &mut cli, &call).await;
	let generation = controller_of(&call_in(&mut cli, &call).await.expect("call")).generation;
	Talk {
		cli,
		call,
		id,
		generation,
	}
}

fn route(talk: &Talk, op: u8, mode: TransmitMode, consent: bool) -> SetTransmitRouteRequest {
	SetTransmitRouteRequest {
		context: Some(ctx(&talk.cli, op)),
		call_id: talk.call.clone(),
		controller_generation: talk.generation,
		mode: mode as i32,
		explicit_microphone_consent: consent,
		authorize_background_open_mic: false,
	}
}

/// Grants push-to-talk consent and completes the capture open.
async fn grant_ptt(rig: &Rig, talk: &mut Talk) {
	let request = route(talk, 2, TransmitMode::PushToTalk, true);
	assert_applied(
		&talk
			.cli
			.raw()
			.set_transmit_route(request)
			.await
			.expect("route")
			.into_inner(),
	);
	let generation = eventually!(rig.fakes.media.calls().into_iter().find_map(|c| match c {
		MediaCall::OpenCapture(generation) => Some(generation),
		_ => None,
	}));
	rig.daemon
		.control()
		.system(SystemEvent::CaptureReady { generation })
		.await
		.expect("ready");
}

fn press(talk: &Talk, sequence: u64, action: InputAction) -> InputControlMessage {
	InputControlMessage {
		client: talk.cli.context(),
		source_id: vec![1; 16],
		input_sequence: sequence,
		call_id: talk.call.clone(),
		controller_generation: talk.generation,
		transmit_route_generation: 0,
		action: action as i32,
		requested_lease_ms: 300,
	}
}

async fn open_input(
	talk: &mut Talk,
) -> (
	mpsc::Sender<InputControlMessage>,
	Streaming<InputControlAck>,
) {
	let (tx, rx) = mpsc::channel(8);
	let acks = talk
		.cli
		.raw()
		.input_control(ReceiverStream::new(rx))
		.await
		.expect("input stream")
		.into_inner();
	(tx, acks)
}

#[tokio::test]
async fn consent_must_be_explicit() {
	let rig = Rig::start().await;
	let mut talk = connected(&rig).await;
	let request = route(&talk, 2, TransmitMode::PushToTalk, false);
	let error = talk
		.cli
		.raw()
		.set_transmit_route(request)
		.await
		.expect_err("no consent");
	assert_eq!(error.code(), Code::InvalidArgument);
	let request = route(&talk, 3, TransmitMode::Unspecified, true);
	let error = talk
		.cli
		.raw()
		.set_transmit_route(request)
		.await
		.expect_err("no mode");
	assert_eq!(error.code(), Code::InvalidArgument);
	assert!(!rig.daemon.gates().load().is_open(talk.id));
	assert!(
		rig.fakes
			.media
			.calls()
			.iter()
			.all(|c| !matches!(c, MediaCall::OpenCapture(_)))
	);
}

#[tokio::test]
async fn a_held_press_opens_the_gate_and_closing_the_stream_closes_it() {
	let rig = Rig::start().await;
	let mut talk = connected(&rig).await;
	grant_ptt(&rig, &mut talk).await;
	assert!(
		!rig.daemon.gates().load().is_open(talk.id),
		"consent alone never transmits"
	);

	let (tx, mut acks) = open_input(&mut talk).await;
	tx.send(press(&talk, 1, InputAction::Press))
		.await
		.expect("press");
	let ack = acks.message().await.expect("ack").expect("open");
	assert!(ack.accepted, "{ack:?}");
	assert!(ack.effective_transmitting);
	assert!(ack.effective_lease_ms > 0 && ack.effective_lease_ms <= 500);
	assert!(rig.daemon.gates().load().is_open(talk.id));

	// A stale sequence is refused and does not disturb the lease.
	tx.send(press(&talk, 1, InputAction::Renew))
		.await
		.expect("stale");
	let ack = acks.message().await.expect("ack").expect("open");
	assert!(!ack.accepted);
	assert_eq!(ack.problem.expect("problem").code, "STALE_SEQUENCE");

	drop(tx);
	eventually!((!rig.daemon.gates().load().is_open(talk.id)).then_some(()));
}

#[tokio::test]
async fn explicit_release_and_global_mute_close_the_gate() {
	let rig = Rig::start().await;
	let mut talk = connected(&rig).await;
	grant_ptt(&rig, &mut talk).await;
	let (tx, mut acks) = open_input(&mut talk).await;
	tx.send(press(&talk, 1, InputAction::Press))
		.await
		.expect("press");
	acks.message().await.expect("ack");
	assert!(rig.daemon.gates().load().is_open(talk.id));

	tx.send(press(&talk, 2, InputAction::Release))
		.await
		.expect("release");
	let ack = acks.message().await.expect("ack").expect("open");
	assert!(ack.accepted && !ack.effective_transmitting);
	assert!(!rig.daemon.gates().load().is_open(talk.id));

	tx.send(press(&talk, 3, InputAction::Press))
		.await
		.expect("press again");
	acks.message().await.expect("ack");
	assert!(rig.daemon.gates().load().is_open(talk.id));
	let request = SetGlobalControlsRequest {
		context: Some(ctx(&talk.cli, 9)),
		muted: Some(true),
		deafened: None,
		microphone_gain: None,
		playback_gain: None,
		ptt_release_delay_ms: None,
	};
	talk.cli
		.raw()
		.set_global_controls(request)
		.await
		.expect("mute");
	// Acknowledged means already closed: the gate was published first.
	assert!(!rig.daemon.gates().load().is_open(talk.id));
}

#[tokio::test]
async fn takeover_drops_the_previous_holders_press() {
	let rig = Rig::start().await;
	let mut talk = connected(&rig).await;
	grant_ptt(&rig, &mut talk).await;
	let (tx, mut acks) = open_input(&mut talk).await;
	tx.send(press(&talk, 1, InputAction::Press))
		.await
		.expect("press");
	acks.message().await.expect("ack");
	assert!(rig.daemon.gates().load().is_open(talk.id));

	let mut gui = rig.full_client("desktop").await;
	let request = AcquireControllerRequest {
		context: Some(ctx(&gui, 3)),
		call_id: talk.call.clone(),
		request_explicit_takeover: true,
	};
	gui.raw()
		.acquire_controller(request)
		.await
		.expect("takeover");
	assert!(!rig.daemon.gates().load().is_open(talk.id));
	let route = call_in(&mut gui, &talk.call)
		.await
		.expect("call")
		.transmit
		.expect("route");
	assert!(
		!route.consent_granted,
		"takeover resets the route to receive-only"
	);
}

#[tokio::test]
async fn capture_loss_fails_closed_until_resumed() {
	let rig = Rig::start().await;
	let mut talk = connected(&rig).await;
	grant_ptt(&rig, &mut talk).await;
	let (tx, mut acks) = open_input(&mut talk).await;
	tx.send(press(&talk, 1, InputAction::Press))
		.await
		.expect("press");
	acks.message().await.expect("ack");
	let generation = rig
		.daemon
		.gates()
		.load()
		.get(talk.id)
		.expect("gate")
		.capture;
	rig.daemon
		.control()
		.system(SystemEvent::CaptureLost { generation })
		.await
		.expect("lost");
	eventually!((!rig.daemon.gates().load().is_open(talk.id)).then_some(()));
}
