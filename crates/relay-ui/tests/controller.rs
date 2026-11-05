mod common;

use common::*;
use relay_api::pb::{self, CallState};
use relay_ui::controller::{AVATAR_MAX_BYTES, ControlError, Controller, SessionSlot};
use relay_ui::engine::Command;
use relay_ui::hub::Hub;
use relay_ui::model::Mode;
use std::sync::Arc;

fn call_hex() -> String {
	hex::encode(CALL)
}

fn mine() -> pb::Snapshot {
	snapshot(5, vec![pb_call(CallState::Connected, Some((&ME, 3)))])
}

fn foreign() -> pb::Snapshot {
	snapshot(5, vec![pb_call(CallState::Connected, Some((&[9; 16], 2)))])
}

#[tokio::test]
async fn commands_fail_cleanly_without_a_daemon() {
	let hub = Arc::new(Hub::default());
	let ctl = Controller::new(hub, Arc::new(SessionSlot::default()));
	assert_eq!(
		ctl.set_profile("Ada").await,
		Err(ControlError::NotConnected)
	);
}

#[tokio::test]
async fn uses_the_held_lease_generation() {
	let (ctl, _hub, ops) = controller_with(mine(), 99);
	ctl.end_call(&call_hex()).await.unwrap();
	assert_eq!(
		ops.recorded(),
		vec![Command::EndCall {
			call_id: CALL.to_vec(),
			generation: 3
		}]
	);
}

#[tokio::test]
async fn acquires_control_before_mutating_a_call_held_elsewhere() {
	let (ctl, _hub, ops) = controller_with(foreign(), 42);
	ctl.accept_call(&call_hex()).await.unwrap();
	assert_eq!(
		ops.recorded(),
		vec![
			Command::AcquireController {
				call_id: CALL.to_vec()
			},
			Command::AcceptCall {
				call_id: CALL.to_vec(),
				generation: 42
			},
		]
	);
}

#[tokio::test]
async fn transmit_requires_explicit_consent() {
	let (ctl, _hub, ops) = controller_with(mine(), 0);
	let err = ctl
		.set_transmit(&call_hex(), Mode::OpenMic, false)
		.await
		.unwrap_err();
	assert!(matches!(err, ControlError::Invalid(_)));
	assert!(ops.recorded().is_empty());

	ctl.set_transmit(&call_hex(), Mode::PushToTalk, true)
		.await
		.unwrap();
	ctl.set_transmit(&call_hex(), Mode::ReceiveOnly, true)
		.await
		.unwrap();
	let sent = ops.recorded();
	assert_eq!(
		sent[0],
		Command::SetTransmit {
			call_id: CALL.to_vec(),
			generation: 3,
			mode: Mode::PushToTalk,
			consent: true
		}
	);
	// Listen-only never carries consent, whatever the caller passed.
	assert_eq!(
		sent[1],
		Command::SetTransmit {
			call_id: CALL.to_vec(),
			generation: 3,
			mode: Mode::ReceiveOnly,
			consent: false
		}
	);
}

#[tokio::test]
async fn rejects_malformed_and_unknown_ids() {
	let (ctl, _hub, ops) = controller_with(mine(), 0);
	assert!(ctl.end_call("zz").await.is_err());
	assert!(ctl.end_call(&hex::encode([5u8; 16])).await.is_err());
	assert!(ctl.start_call(&hex::encode([5u8; 32])).await.is_err());
	assert!(ctl.start_call("short").await.is_err());
	assert!(ops.recorded().is_empty());
	ctl.start_call(&hex::encode(PEER)).await.unwrap();
	assert_eq!(
		ops.recorded(),
		vec![Command::StartCall {
			peer_id: PEER.to_vec()
		}]
	);
}

#[tokio::test]
async fn validates_gains_names_devices_and_avatars() {
	let (ctl, _hub, ops) = controller_with(mine(), 0);
	assert!(
		ctl.set_global_controls(None, None, Some(f32::NAN), None)
			.await
			.is_err()
	);
	assert!(
		ctl.set_global_controls(None, None, None, Some(9.0))
			.await
			.is_err()
	);
	assert!(
		ctl.set_call_controls(&call_hex(), None, None, Some(-1.0))
			.await
			.is_err()
	);
	assert!(ctl.set_profile("  ").await.is_err());
	assert!(ctl.set_profile(&"x".repeat(65)).await.is_err());
	assert!(ctl.set_profile("bad\u{7}name").await.is_err());
	assert!(
		ctl.select_devices("mic", "gone").await.is_err(),
		"unavailable device"
	);
	assert!(ctl.select_devices("nope", "spk").await.is_err());
	assert!(ctl.upload_avatar(vec![1], "image/gif").await.is_err());
	assert!(ctl.upload_avatar(Vec::new(), "image/png").await.is_err());
	assert!(
		ctl.upload_avatar(vec![0; AVATAR_MAX_BYTES + 1], "image/png")
			.await
			.is_err()
	);
	assert!(ops.recorded().is_empty());

	ctl.set_profile(" Ada ").await.unwrap();
	ctl.select_devices("mic", "spk").await.unwrap();
	ctl.upload_avatar(vec![0; AVATAR_MAX_BYTES], "image/png")
		.await
		.unwrap();
	assert_eq!(ops.recorded().len(), 3);
	assert_eq!(
		ops.recorded()[0],
		Command::SetProfile {
			display_name: "Ada".into()
		}
	);
}

#[tokio::test]
async fn engine_rejections_reach_the_caller_as_text() {
	let (ctl, _hub, ops) = controller_with(mine(), 0);
	*ops.fail_with.lock().unwrap() = Some("call is ending".into());
	let err = ctl.end_call(&call_hex()).await.unwrap_err();
	assert_eq!(serde_json::to_string(&err).unwrap(), "\"call is ending\"");
}

#[tokio::test]
async fn release_delay_is_bounded_and_sent_as_a_global_control() {
	let (ctl, _hub, ops) = controller_with(mine(), 0);
	assert!(ctl.set_ptt_release_delay(2001).await.is_err());
	assert!(ops.recorded().is_empty());
	ctl.set_ptt_release_delay(0).await.unwrap();
	ctl.set_ptt_release_delay(2000).await.unwrap();
	let sent: Vec<_> = ops
		.recorded()
		.into_iter()
		.map(|c| match c {
			Command::SetGlobalControls {
				ptt_release_delay_ms,
				muted: None,
				deafened: None,
				microphone_gain: None,
				playback_gain: None,
			} => ptt_release_delay_ms,
			other => panic!("{other:?}"),
		})
		.collect();
	assert_eq!(sent, vec![Some(0), Some(2000)]);
}

#[tokio::test]
async fn ring_preferences_are_bounded_and_preview_is_a_command() {
	let (ctl, _hub, ops) = controller_with(mine(), 0);
	assert!(
		ctl.set_ring_preferences(None, Some(1.5), None, None)
			.await
			.is_err()
	);
	assert!(
		ctl.set_ring_preferences(None, Some(f32::NAN), None, None)
			.await
			.is_err()
	);
	assert!(ops.recorded().is_empty());
	ctl.set_ring_preferences(Some(false), Some(0.25), None, None)
		.await
		.unwrap();
	ctl.preview_tone().await.unwrap();
	assert_eq!(
		ops.recorded(),
		vec![
			Command::SetRingPreferences {
				ringtone_enabled: Some(false),
				ringtone_volume: Some(0.25),
				ringback_enabled: None,
				call_sounds_enabled: None,
			},
			Command::PreviewTone,
		]
	);
}
