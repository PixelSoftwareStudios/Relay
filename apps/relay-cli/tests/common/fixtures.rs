//! A small, stable engine state used across tests.

use relay_api::pb::*;

pub const ALICE: [u8; 32] = [0xaa; 32];
pub const BOB: [u8; 32] = [0xbb; 32];
pub const CALL: [u8; 16] = [0xc1; 16];
pub const CHANNEL_A: [u8; 16] = [0xa1; 16];
pub const CHANNEL_B: [u8; 16] = [0xb2; 16];

pub fn channel(name: &str, id: [u8; 16], role: &str) -> ChannelInfo {
	ChannelInfo {
		channel_id: id.to_vec(),
		name: name.into(),
		policy: Some(ChannelPolicy {
			listed: true,
			password: true,
			invite_only: false,
		}),
		role: role.into(),
		shared_peers: 2,
		cert_expires_unix: 0,
	}
}

pub fn peer(id: [u8; 32], name: &str, reachable: bool, trust: TrustState) -> Peer {
	Peer {
		peer_id: id.to_vec(),
		display_name: name.into(),
		local_addresses: vec!["192.168.1.20:7000".into()],
		trust: trust as i32,
		reachable,
		avatar_sha256: Vec::new(),
		fingerprint_short: String::new(),
		shared_channel_ids: Vec::new(),
		..Peer::default()
	}
}

pub fn device(id: &str, name: &str, direction: DeviceDirection) -> Device {
	Device {
		device_id: id.into(),
		name: name.into(),
		direction: direction as i32,
		available: true,
		system_default: id.ends_with('1'),
		supported_rates_hint: vec![48_000],
	}
}

pub fn call(state: CallState, mode: TransmitMode) -> Call {
	Call {
		call_id: CALL.to_vec(),
		peer_id: ALICE.to_vec(),
		state: state as i32,
		revision: 3,
		transmit: Some(TransmitRoute {
			mode: mode as i32,
			generation: 7,
			..TransmitRoute::default()
		}),
		receive_gain: 1.0,
		..Call::default()
	}
}

/// Alice (online, verified) and Bob (offline); two mics, one speaker; one live call.
pub fn snapshot() -> Snapshot {
	Snapshot {
		engine_incarnation: vec![1; 16],
		revision: 10,
		peers: vec![
			peer(ALICE, "Alice", true, TrustState::Verified),
			peer(BOB, "Bob", false, TrustState::TofuUnverified),
		],
		calls: vec![call(CallState::Connected, TransmitMode::Muted)],
		devices: vec![
			device("mic1", "Built-in Mic", DeviceDirection::Input),
			device("mic2", "USB Mic", DeviceDirection::Input),
			device("spk1", "Speakers", DeviceDirection::Output),
		],
		settings: Some(Settings {
			revision: 1,
			input_device_id: "mic1".into(),
			output_device_id: "spk1".into(),
			noise_suppression_enabled: true,
			noise_suppression_level: "moderate".into(),
			background_available: true,
			call_alert: "system".into(),
			auto_answer_enabled: true,
			microphone_gain: 1.0,
			playback_gain: 1.0,
			..Settings::default()
		}),
		warnings: Vec::new(),
		channels: vec![
			channel("Dorm", CHANNEL_A, "owner"),
			channel("Study", CHANNEL_B, "member"),
		],
		nearby_channels: Vec::new(),
	}
}

pub fn no_calls() -> Snapshot {
	Snapshot {
		calls: Vec::new(),
		..snapshot()
	}
}
