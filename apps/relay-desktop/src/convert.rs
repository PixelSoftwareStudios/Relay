//! Wire (`relay_api::pb`) to view-model conversion. Unknown enum values map
//! to the most conservative reading rather than failing the whole stream.

use relay_api::pb;

use crate::model::{
	Call, CallMeter, Device, Direction, EngineInfo, MAX_ITEMS, Meters, Mode, Peer, Phase, Problem,
	Settings, Transmit, Trust,
};

pub fn trust(value: i32) -> Trust {
	match pb::TrustState::try_from(value) {
		Ok(pb::TrustState::TofuUnverified) => Trust::TofuUnverified,
		Ok(pb::TrustState::Verified) => Trust::Verified,
		Ok(pb::TrustState::KeyChanged) => Trust::KeyChanged,
		Ok(pb::TrustState::Blocked) => Trust::Blocked,
		_ => Trust::Unknown,
	}
}

pub fn phase(value: i32) -> Phase {
	use pb::CallState as S;
	match S::try_from(value) {
		Ok(S::OutgoingRinging) => Phase::OutgoingRinging,
		Ok(S::IncomingRinging) => Phase::IncomingRinging,
		Ok(S::Negotiating) => Phase::Negotiating,
		Ok(S::Connected) => Phase::Connected,
		Ok(S::Interrupted) => Phase::Interrupted,
		Ok(S::Ending) => Phase::Ending,
		Ok(S::Ended) => Phase::Ended,
		Ok(S::Failed) => Phase::Failed,
		Ok(S::Declined) => Phase::Declined,
		Ok(S::Cancelled) => Phase::Cancelled,
		_ => Phase::Preparing,
	}
}

pub fn mode(value: i32) -> Mode {
	match pb::TransmitMode::try_from(value) {
		Ok(pb::TransmitMode::PushToTalk) => Mode::PushToTalk,
		Ok(pb::TransmitMode::OpenMic) => Mode::OpenMic,
		_ => Mode::ReceiveOnly,
	}
}

pub fn mode_to_wire(mode: Mode) -> pb::TransmitMode {
	match mode {
		Mode::ReceiveOnly => pb::TransmitMode::ReceiveOnly,
		Mode::PushToTalk => pb::TransmitMode::PushToTalk,
		Mode::OpenMic => pb::TransmitMode::OpenMic,
	}
}

pub fn problem(p: &pb::Problem) -> Problem {
	Problem {
		code: p.code.clone(),
		message: p.message.clone(),
		retryable: p.retryable,
		recovery_action: p.recovery_action.clone(),
	}
}

pub fn peer(p: &pb::Peer) -> Peer {
	Peer {
		peer_id: hex::encode(&p.peer_id),
		display_name: p.display_name.clone(),
		addresses: p.local_addresses.iter().take(8).cloned().collect(),
		trust: trust(p.trust),
		reachable: p.reachable,
		has_avatar: !p.avatar_sha256.is_empty(),
	}
}

pub fn call(c: &pb::Call, me: &[u8]) -> Call {
	let transmit = c.transmit.clone().unwrap_or_default();
	let lease = c.controller.as_ref();
	Call {
		call_id: hex::encode(&c.call_id),
		peer_id: hex::encode(&c.peer_id),
		phase: phase(c.state),
		transmit: Transmit {
			mode: mode(transmit.mode),
			consent_granted: transmit.consent_granted,
			effective_transmitting: transmit.effective_transmitting,
			blocked_reason: transmit.blocked_reason,
		},
		user_muted: c.user_muted,
		user_deafened: c.user_deafened,
		receive_gain: c.receive_gain,
		profile: c.negotiated_profile.clone(),
		problem: c.problem.as_ref().map(problem),
		controlled_by_me: lease.is_some_and(|l| !me.is_empty() && l.controller_client_id == me),
		controller_generation: lease.map_or(0, |l| l.generation),
	}
}

pub fn device(d: &pb::Device) -> Device {
	Device {
		device_id: d.device_id.clone(),
		name: d.name.clone(),
		direction: if d.direction == pb::DeviceDirection::Output as i32 {
			Direction::Output
		} else {
			Direction::Input
		},
		available: d.available,
		system_default: d.system_default,
	}
}

pub fn settings(s: &pb::Settings) -> Settings {
	Settings {
		input_device_id: s.input_device_id.clone(),
		output_device_id: s.output_device_id.clone(),
		global_muted: s.global_muted,
		global_deafened: s.global_deafened,
		microphone_gain: s.microphone_gain,
		playback_gain: s.playback_gain,
	}
}

pub fn engine_info(i: &pb::EngineInfo) -> EngineInfo {
	EngineInfo {
		version: i.application_version.clone(),
		api_major: i.api_major,
		api_minor: i.api_minor,
	}
}

/// Finite readings only; NaN and infinities are dropped as "no data".
fn finite(value: Option<f32>) -> Option<f32> {
	value.filter(|v| v.is_finite())
}

pub fn meters(t: &pb::TelemetrySnapshot) -> Meters {
	Meters {
		seq: t.sample_sequence,
		microphone_dbfs: finite(t.microphone_dbfs),
		calls: t
			.calls
			.iter()
			.take(MAX_ITEMS)
			.map(|c| CallMeter {
				call_id: hex::encode(&c.call_id),
				speaking: c.speaking,
				receive_dbfs: finite(c.receive_dbfs),
				rtt_ms: c.rtt_ms.filter(|v| v.is_finite()),
			})
			.collect(),
	}
}
