//! Wire (`relay_api::pb`) to view-model conversion. Unknown enum values map
//! to the most conservative reading rather than failing the whole stream.

use relay_api::pb;

use crate::model::{
	Advert, Call, CallMeter, Channel, ConnectionView, Device, Direction, EngineInfo,
	HistoryDirection, HistoryEntry, HistoryOutcome, Inbound, MAX_ITEMS, Meters, Mode, Peer, Phase,
	Problem, Settings, Transmit, Trust,
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
		Ok(pb::TransmitMode::Unmuted) => Mode::Unmuted,
		_ => Mode::Muted,
	}
}

pub fn mode_to_wire(mode: Mode) -> pb::TransmitMode {
	match mode {
		Mode::Muted => pb::TransmitMode::Muted,
		Mode::PushToTalk => pb::TransmitMode::PushToTalk,
		Mode::Unmuted => pb::TransmitMode::Unmuted,
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

/// The short key digest as shown to the user. It comes from the engine, so only hex digits
/// and dashes survive, within the length of the documented form.
fn short_fingerprint(text: &str) -> String {
	text.chars()
		.filter(|c| c.is_ascii_hexdigit() || *c == '-')
		.take(24)
		.collect::<String>()
		.to_ascii_uppercase()
}

pub fn peer(p: &pb::Peer) -> Peer {
	Peer {
		peer_id: hex::encode(&p.peer_id),
		display_name: p.display_name.clone(),
		addresses: p.local_addresses.iter().take(8).cloned().collect(),
		trust: trust(p.trust),
		reachable: p.reachable,
		avatar_hash: hex::encode(&p.avatar_sha256),
		fingerprint_short: short_fingerprint(&p.fingerprint_short),
		port: p.port,
		last_seen_unix_ms: p.last_seen_unix_ms,
		shared_channels: p
			.shared_channel_ids
			.iter()
			.take(32)
			.map(hex::encode)
			.collect(),
		favorite: p.favorite,
		favorite_position: p.favorite_position,
		key_changed: p.key_changed,
		previous_peer_id: hex::encode(&p.previous_peer_id),
		previous_fingerprint_short: short_fingerprint(&p.previous_fingerprint_short),
		auto_answer: p.auto_answer,
	}
}

pub fn channel(c: &pb::ChannelInfo) -> Channel {
	let policy = c.policy.unwrap_or_default();
	Channel {
		channel_id: hex::encode(&c.channel_id),
		name: c.name.clone(),
		listed: policy.listed,
		password: policy.password,
		invite_only: policy.invite_only,
		role: c.role.clone(),
		shared_peers: c.shared_peers,
		cert_expires_unix: c.cert_expires_unix,
	}
}

pub fn connection(i: &pb::ConnectionInfo) -> ConnectionView {
	ConnectionView {
		addresses: i.local_addresses.iter().take(16).cloned().collect(),
		port: i.listen_port,
		fingerprint: hex::encode(&i.fingerprint),
		connection_string: i.connection_string.clone(),
	}
}

pub fn inbound_to_wire(v: Inbound) -> pb::InboundPolicy {
	match v {
		Inbound::Anyone => pb::InboundPolicy::Anyone,
		Inbound::SharedChannelOnly => pb::InboundPolicy::SharedChannelOnly,
	}
}

pub fn advert_to_wire(v: Advert) -> pb::AdvertMode {
	match v {
		Advert::Visible => pb::AdvertMode::Visible,
		Advert::Hidden => pb::AdvertMode::Hidden,
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
			releasing: transmit.releasing,
		},
		user_muted: c.user_muted,
		user_deafened: c.user_deafened,
		receive_gain: c.receive_gain,
		profile: c.negotiated_profile.clone(),
		problem: c.problem.as_ref().map(problem),
		controlled_by_me: lease.is_some_and(|l| !me.is_empty() && l.controller_client_id == me),
		controller_generation: lease.map_or(0, |l| l.generation),
		connected_at_unix_ms: c.connected_at_unix_ms,
		auto_answered: c.auto_answered,
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
		input_is_default: s.input_is_default,
		output_is_default: s.output_is_default,
		input_device_name: s.input_device_name.clone(),
		output_device_name: s.output_device_name.clone(),
		input_pinned_missing: s.input_pinned_missing,
		output_pinned_missing: s.output_pinned_missing,
		global_muted: s.global_muted,
		global_deafened: s.global_deafened,
		microphone_gain: s.microphone_gain,
		playback_gain: s.playback_gain,
		ptt_release_delay_ms: s.ptt_release_delay_ms,
		autostart_daemon: s.autostart_daemon,
		ringtone_enabled: s.ringtone_enabled,
		ringtone_volume: s.ringtone_volume,
		ringback_enabled: s.ringback_enabled,
		call_sounds_enabled: s.call_sounds_enabled,
		noise_suppression_enabled: s.noise_suppression_enabled,
		noise_suppression_level: crate::audio_processing::NoiseLevel::from_key(
			&s.noise_suppression_level,
		),
		auto_gain_enabled: s.auto_gain_enabled,
		ringtone_name: s.ringtone_name.clone(),
		history_enabled: s.history_enabled,
		history_revision: s.history_revision,
		default_transmit_mode: mode(s.default_transmit_mode),
		background_available: s.background_available,
		call_alert: crate::model::CallAlert::from_key(&s.call_alert),
		auto_answer_enabled: s.auto_answer_enabled,
		daemon_build_revision: s.daemon_build_revision.clone(),
		daemon_api_revision: s.daemon_api_revision,
		daemon_version: s.daemon_version.clone(),
		display_name: s.display_name.clone(),
		avatar_hash: hex::encode(&s.avatar_sha256),
		// An unspecified value is the conservative reading.
		inbound: if s.inbound_policy == pb::InboundPolicy::Anyone as i32 {
			Inbound::Anyone
		} else {
			Inbound::SharedChannelOnly
		},
		advert: if s.advert_mode == pb::AdvertMode::Visible as i32 {
			Advert::Visible
		} else {
			Advert::Hidden
		},
	}
}

/// A call history entry. An unknown direction or outcome reads as the plainest case.
pub fn history_entry(e: &pb::CallHistoryEntry) -> HistoryEntry {
	HistoryEntry {
		id: e.id,
		call_id: hex::encode(&e.call_id),
		peer_id: hex::encode(&e.peer_id),
		peer_name: e.peer_name.clone(),
		fingerprint_short: short_fingerprint(&e.peer_fingerprint_short),
		direction: if e.direction == pb::CallDirection::Incoming as i32 {
			HistoryDirection::Incoming
		} else {
			HistoryDirection::Outgoing
		},
		outcome: match pb::CallOutcome::try_from(e.outcome) {
			Ok(pb::CallOutcome::Missed) => HistoryOutcome::Missed,
			Ok(pb::CallOutcome::Declined) => HistoryOutcome::Declined,
			Ok(pb::CallOutcome::Failed) => HistoryOutcome::Failed,
			Ok(pb::CallOutcome::Cancelled) => HistoryOutcome::Cancelled,
			_ => HistoryOutcome::Completed,
		},
		started_at_ms: e.started_at_unix_ms,
		duration_ms: e.duration_ms,
	}
}

pub fn startup(s: &pb::Startup) -> crate::model::Startup {
	crate::model::Startup {
		failed: s.state() == pb::StartupState::Failed,
		unreadable: s.reason() == pb::StartupReason::IdentityUnreadable,
		detail: s.detail.clone(),
	}
}

pub fn engine_info(i: &pb::EngineInfo) -> EngineInfo {
	EngineInfo {
		version: i.application_version.clone(),
		api_major: i.api_major,
		api_minor: i.api_minor,
		build_revision: i.build_revision.clone(),
		api_revision: i.api_revision,
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

/// One call-statistics sample. Non-finite readings are dropped as "no data".
pub fn call_stats(s: &pb::CallStats) -> crate::model::CallStats {
	let finite = |v: Option<f64>| v.filter(|x| x.is_finite());
	let number = |v: f64| if v.is_finite() { v } else { 0.0 };
	crate::model::CallStats {
		call_id: hex::encode(&s.call_id),
		path: match pb::MediaPathState::try_from(s.path_state) {
			Ok(pb::MediaPathState::Connected) => crate::model::MediaPath::Connected,
			Ok(pb::MediaPathState::Failed) => crate::model::MediaPath::Failed,
			_ => crate::model::MediaPath::Waiting,
		},
		path_reason: s.path_reason.chars().take(200).collect(),
		sending: s.sending,
		receiving: s.receiving,
		packets_sent: s.packets_sent,
		packets_received: s.packets_received,
		mic_level_dbfs: finite(s.mic_level_dbfs.map(f64::from)).map(|v| v as f32),
		rtt_ms: finite(s.rtt_ms),
		rtt_min_ms: finite(s.rtt_min_ms),
		rtt_max_ms: finite(s.rtt_max_ms),
		jitter_ms: finite(s.jitter_ms),
		loss_percent_5s: number(s.loss_percent_5s),
		loss_percent_total: number(s.loss_percent_total),
		jitter_buffer_target_ms: number(s.jitter_buffer_target_ms),
		jitter_buffer_depth_ms: number(s.jitter_buffer_depth_ms),
		playout_delay_ms: number(s.playout_delay_ms),
		mouth_to_ear_ms: finite(s.mouth_to_ear_ms),
		codec_bitrate_bps: s.codec_bitrate_bps,
		packet_bytes: s.packet_bytes,
		local_addr: s.local_addr.chars().take(64).collect(),
		remote_addr: s.remote_addr.chars().take(64).collect(),
		transport: s.transport.chars().take(16).collect(),
		clock_drift_ppm: finite(s.clock_drift_ppm),
		capture_buffer_frames: s.capture_buffer_frames,
		capture_buffer_ms: number(s.capture_buffer_ms),
		playback_buffer_frames: s.playback_buffer_frames,
		playback_buffer_ms: number(s.playback_buffer_ms),
	}
}
