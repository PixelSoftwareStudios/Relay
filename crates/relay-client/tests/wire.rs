#![cfg(unix)]

mod support;

use relay_api::pb::{OperationState, TransmitMode, TrustState};
use relay_client::wire::{acquire_generation, connect_registered, open_telemetry, send};
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
async fn consent_travels_in_the_same_request_as_the_route() {
	let h = Harness::start().await;
	let mut c = client(&h).await;
	send(
		&mut c,
		Command::SetTransmit {
			call_id: vec![1; 16],
			generation: 2,
			mode: TransmitMode::OpenMic,
			consent: true,
		},
	)
	.await
	.unwrap();
	assert_eq!(
		h.log(),
		[format!(
			"route mode={} consent=true gen=2",
			TransmitMode::OpenMic as i32
		)]
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
		},
		Command::SelectDevices {
			input_id: "mic".into(),
			output_id: "spk".into(),
		},
		Command::SetNoiseSuppression {
			backend: "rnn".into(),
		},
		Command::SetProfile {
			display_name: "Ada".into(),
		},
		Command::UploadAvatar {
			bytes: vec![0; 3],
			media_type: "image/png".into(),
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
			"global muted=None mic=Some(2.0)".into(),
			"devices mic spk".into(),
			"ns rnn".into(),
			"profile Ada".into(),
			"avatar 3 image/png".into(),
			"mic_test_start".into(),
			"mic_test_stop".into(),
			format!("trust {} oob=true", TrustState::Verified as i32),
		]
	);
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
		]
	);
}
