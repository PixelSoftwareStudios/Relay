#![cfg(unix)]

mod support;

use relay_api::pb::{OperationState, TransmitMode, TrustState};
use relay_client::wire::{
	acquire_generation, connect_registered, open_call_stats, open_telemetry, send,
};
use relay_client::{CallAction, CallTarget, ChannelOptions, Command, Secret};
use support::Harness;

async fn client(h: &Harness) -> relay_api::RelayClient {
	connect_registered(&h.endpoint(), "test", "0", &["observe", "call-control"])
		.await
		.unwrap()
}

#[tokio::test]
async fn call_bound_commands_carry_the_given_generation() {
	let h = Harness::start().await;
	let mut c = client(&h).await;
	let call_id = vec![1; 16];
	let generation = acquire_generation(&mut c, &call_id, true).await.unwrap();
	assert_eq!(generation, 5);
	for action in [CallAction::Accept, CallAction::Decline, CallAction::End] {
		let op = send(
			&mut c,
			Command::CallAction {
				action,
				call_id: call_id.clone(),
				generation,
			},
		)
		.await
		.unwrap();
		assert_eq!(op.state, OperationState::Applied as i32);
	}
	assert_eq!(
		h.log(),
		[
			"acquire takeover=true",
			"accept gen=5",
			"decline gen=5",
			"end gen=5"
		]
	);
}

#[tokio::test]
async fn the_mode_is_the_consent_and_travels_in_one_request() {
	let h = Harness::start().await;
	let mut c = client(&h).await;
	send(
		&mut c,
		Command::SetTransmit {
			call_id: vec![1; 16],
			generation: 2,
			mode: TransmitMode::Unmuted,
		},
	)
	.await
	.unwrap();
	assert_eq!(
		h.log(),
		[format!("route mode={} gen=2", TransmitMode::Unmuted as i32)]
	);
}

#[tokio::test]
async fn every_other_command_reaches_its_rpc() {
	let h = Harness::start().await;
	let mut c = client(&h).await;
	let commands = [
		Command::StartCall(CallTarget::Peer(vec![0xaa; 2])),
		Command::StartCall(CallTarget::Address("10.0.0.2:7000".into())),
		Command::SetCallControls {
			call_id: vec![1],
			generation: 1,
			muted: Some(true),
			deafened: None,
			receive_gain: Some(0.5),
		},
		Command::SetGlobalControls {
			muted: None,
			deafened: Some(false),
			microphone_gain: Some(2.0),
			playback_gain: None,
			ptt_release_delay_ms: Some(150),
		},
		Command::SetRingPreferences {
			ringtone_enabled: Some(false),
			ringtone_volume: Some(0.5),
			ringback_enabled: None,
			call_sounds_enabled: Some(true),
		},
		Command::PreviewTone,
		Command::SetRingtone {
			file_name: "tone.mp3".into(),
			data: vec![0; 4],
		},
		Command::ResetRingtone,
		Command::SelectDevices {
			input_id: "mic".into(),
			output_id: "spk".into(),
		},
		Command::SetProfile {
			display_name: "Ada".into(),
		},
		Command::UploadAvatar {
			bytes: vec![0; 3],
			media_type: "image/png".into(),
		},
		Command::ClearAvatar,
		Command::ClearCallHistory,
		Command::ForgetPeer {
			peer_id: Vec::new(),
			query: "kitchen".into(),
		},
		Command::SetFavorites {
			peer_ids: vec![vec![0xab; 32], vec![0xcd; 32]],
		},
		Command::SetPeerAutoAnswer {
			peer_id: vec![0xab; 32],
			enabled: true,
		},
		Command::SetSettings {
			values: vec![("hotkeys.push_to_talk".into(), "F9".into())],
		},
		Command::StartMicTest {
			local_playback: false,
			confirmed_feedback: false,
		},
		Command::StopMicTest,
		Command::DecideTrust {
			peer_id: vec![1; 32],
			public_key_sha256: vec![2; 32],
			requested: TrustState::Verified,
			out_of_band_verified: true,
		},
		Command::ConnectAndTrust {
			connection_string: "relay://10.0.0.3:47800#abcd".into(),
		},
	];
	for command in commands {
		send(&mut c, command).await.unwrap();
	}
	assert_eq!(
		h.log(),
		[
			"start_call peer=aaaa".to_owned(),
			"start_call addr=10.0.0.2:7000".into(),
			"call_controls muted=Some(true) gain=Some(0.5)".into(),
			"global muted=None mic=Some(2.0) ptt_delay=Some(150) ring=None/None/None/None".into(),
			"global muted=None mic=None ptt_delay=None ring=Some(false)/Some(0.5)/None/Some(true)"
				.into(),
			"preview_tone".into(),
			"set_ringtone tone.mp3 4".into(),
			"set_ringtone reset".into(),
			"devices mic spk".into(),
			"profile Ada".into(),
			"avatar 3 image/png".into(),
			"clear_avatar".into(),
			"clear_call_history".into(),
			"forget_peer id=0 query=kitchen".into(),
			format!("favorites {},{}", "ab".repeat(32), "cd".repeat(32)),
			"auto_answer ab true".into(),
			"set_settings hotkeys.push_to_talk=F9".into(),
			"mic_test_start".into(),
			"mic_test_stop".into(),
			format!("trust {} oob=true", TrustState::Verified as i32),
			"connect_and_trust relay://10.0.0.3:47800#abcd".into(),
		]
	);
}

#[tokio::test]
async fn call_stats_open_for_one_call() {
	let h = Harness::start().await;
	let mut c = client(&h).await;
	let mut stream = open_call_stats(&mut c, &[7; 16]).await.unwrap();
	let sample = stream.message().await.unwrap().unwrap();
	assert!(sample.sending);
	assert_eq!(h.log(), [format!("call_stats {}", hex::encode([7u8; 16]))]);
}

#[tokio::test]
async fn telemetry_requests_the_given_rates() {
	let h = Harness::start().await;
	let mut c = client(&h).await;
	let mut stream = open_telemetry(&mut c, 10, 1).await.unwrap();
	assert!(stream.message().await.unwrap().is_some());
	assert_eq!(h.log(), ["telemetry 10 1"]);
}

#[tokio::test]
async fn channel_privacy_and_connection_string_commands_reach_the_engine() {
	let h = Harness::start().await;
	let mut c = client(&h).await;
	let commands = [
		Command::StartCall(CallTarget::ConnectionString("relay://1.2.3.4:5#ab".into())),
		Command::CreateChannel {
			name: "dorm".into(),
			options: ChannelOptions {
				listed: true,
				password: true,
				invite_only: false,
			},
			password: Secret::new("pw"),
		},
		Command::JoinChannel {
			channel_id: Vec::new(),
			password: Secret::new(""),
			invite: Secret::new("RLYI1-abc"),
		},
		Command::LeaveChannel {
			channel_id: vec![1; 16],
		},
		Command::RevokeMember {
			channel_id: vec![1; 16],
			fingerprint: vec![2; 32],
		},
		Command::SetPrivacy {
			inbound: Some(relay_api::pb::InboundPolicy::SharedChannelOnly),
			advert: None,
		},
		Command::SetPrivacy {
			inbound: Some(relay_api::pb::InboundPolicy::Nobody),
			advert: None,
		},
	];
	for command in commands {
		let op = send(&mut c, command).await.unwrap();
		assert_eq!(op.state, OperationState::Applied as i32);
	}
	assert_eq!(
		h.log(),
		[
			"start_call conn=relay://1.2.3.4:5#ab",
			"create_channel dorm listed=true pw=true invite=false secret_len=2",
			"join_channel pw=0 invite=9",
			"leave_channel",
			"revoke fp=32",
			"privacy Some(2) None",
			"privacy Some(3) None",
		]
	);
}

#[tokio::test]
async fn messaging_commands_use_their_own_rpcs_and_return_the_inner_operation() {
	let h = Harness::start().await;
	let mut c = connect_registered(&h.endpoint(), "test", "0", &["messages"])
		.await
		.unwrap();
	let session_id = vec![3; 16];
	let sent = Command::SendSessionMessage {
		session_id: session_id.clone(),
		text: "hello".into(),
	};
	assert!(sent.is_messaging());
	let op = send(&mut c, sent).await.unwrap();
	assert_eq!(op.state, OperationState::Applied as i32);
	send(
		&mut c,
		Command::PinMessage {
			session_id,
			outgoing: true,
			seq: 4,
			pinned: true,
		},
	)
	.await
	.unwrap();
	assert_eq!(
		h.log(),
		[
			"send_session_message hello",
			"pin outgoing=true seq=4 pinned=true"
		]
	);
	assert!(!Command::PreviewTone.is_messaging());
}

#[tokio::test]
async fn settings_helpers_report_per_key_outcomes() {
	let h = Harness::start().await;
	let mut c = client(&h).await;
	let answer = c
		.set_settings([
			("ptt.release_delay_ms", "300"),
			("audio.ringtone_volume", "0.5"),
		])
		.await
		.unwrap();
	assert_eq!(answer.results.len(), 2);
	assert!(
		answer
			.results
			.iter()
			.all(|r| r.outcome == relay_api::pb::SettingOutcome::AppliedLive as i32)
	);
	let all = c.get_settings().await.unwrap();
	assert_eq!(
		all.get("ptt.release_delay_ms").map(String::as_str),
		Some("200")
	);
	let forgot = c.forget_peer(Some(&[7; 32]), "").await.unwrap();
	assert_eq!(forgot.state, OperationState::Applied as i32);
	assert_eq!(
		h.log(),
		[
			"set_settings audio.ringtone_volume=0.5,ptt.release_delay_ms=300",
			"get_settings",
			"forget_peer id=32 query="
		]
	);
}
