//! Domain views as wire messages. This is also where the domain's
//! `Connecting`/`Active` become the API's `NEGOTIATING`/`CONNECTED`.

use relay_api::pb::{
	self, AdvertMode, Call, CallState as PbState, ControllerLease, Device,
	DeviceDirection as PbDirection, InboundPolicy, Peer, Settings, Snapshot, StateEvent,
	TransmitMode, TransmitRoute, TrustState, state_event,
};
use relay_core::{CallState, Event, Gain, GlobalView, RouteMode, Snapshot as CoreSnapshot};

use super::lease_bytes;
use crate::service::directory::Directory;
use crate::service::ports::{self, DeviceDirection, DeviceInfo, PeerInfo, PeerTrust};

fn state_to_pb(state: CallState) -> PbState {
	match state {
		CallState::Preparing => PbState::Preparing,
		CallState::OutgoingRinging => PbState::OutgoingRinging,
		CallState::IncomingRinging => PbState::IncomingRinging,
		CallState::Connecting => PbState::Negotiating,
		CallState::Active => PbState::Connected,
		CallState::Interrupted => PbState::Interrupted,
		CallState::Ending => PbState::Ending,
		CallState::Ended => PbState::Ended,
		CallState::Declined => PbState::Declined,
		CallState::Cancelled => PbState::Cancelled,
		CallState::Failed => PbState::Failed,
	}
}

fn mode_to_pb(mode: RouteMode) -> TransmitMode {
	match mode {
		RouteMode::Muted => TransmitMode::Muted,
		RouteMode::PushToTalk => TransmitMode::PushToTalk,
		RouteMode::Unmuted => TransmitMode::Unmuted,
	}
}

fn trust_to_pb(trust: PeerTrust) -> TrustState {
	match trust {
		PeerTrust::Unknown => TrustState::Unknown,
		PeerTrust::TofuUnverified => TrustState::TofuUnverified,
		PeerTrust::Verified => TrustState::Verified,
		PeerTrust::KeyChanged => TrustState::KeyChanged,
		PeerTrust::Blocked => TrustState::Blocked,
	}
}

fn inbound_to_pb(policy: ports::InboundPolicy) -> InboundPolicy {
	match policy {
		ports::InboundPolicy::Anyone => InboundPolicy::Anyone,
		ports::InboundPolicy::SharedChannelOnly => InboundPolicy::SharedChannelOnly,
	}
}

fn advert_to_pb(mode: ports::AdvertMode) -> AdvertMode {
	match mode {
		ports::AdvertMode::Visible => AdvertMode::Visible,
		ports::AdvertMode::Hidden => AdvertMode::Hidden,
	}
}

fn gain_to_f32(gain: Gain) -> f32 {
	f32::from(gain.permille()) / 1000.0
}

/// A call view, with `revision` the API revision it is current at.
pub fn call_to_pb(view: &relay_core::CallView, revision: u64, directory: &Directory) -> Call {
	let controller = view.controller.map(|(client, lease)| ControllerLease {
		call_id: view.id.to_bytes().to_vec(),
		controller_client_id: client.to_bytes().to_vec(),
		generation: lease.get(),
		lease_id: lease_bytes(lease),
		expires_in_ms: 0,
	});
	let blocked = view.gate.deny.map(|deny| words(&format!("{deny:?}")));
	let problem = (view.state == CallState::Failed)
		.then(|| {
			view.reason
				.map(|reason| failure_problem(&format!("{reason:?}")))
		})
		.flatten();
	Call {
		call_id: view.id.to_bytes().to_vec(),
		peer_id: view.peer.as_bytes().to_vec(),
		state: state_to_pb(view.state) as i32,
		revision,
		controller,
		transmit: Some(TransmitRoute {
			mode: mode_to_pb(view.route.mode) as i32,
			consent_granted: view.route.is_authorized(),
			effective_transmitting: view.gate.open,
			blocked_reason: blocked.unwrap_or_default(),
			generation: view.route_generation.get(),
			authorized_background_open_mic: view.route.background,
			releasing: view.releasing,
		}),
		user_muted: view.controls.muted,
		user_deafened: view.controls.deafened,
		receive_gain: gain_to_f32(view.controls.gain),
		negotiated_profile: String::new(),
		problem,
		connected_at_unix_ms: if view.state == CallState::Active {
			directory.connected_at(view.id).unwrap_or(0)
		} else {
			0
		},
	}
}

fn failure_problem(reason: &str) -> pb::Problem {
	pb::Problem {
		code: format!("CALL_FAILED_{}", screaming(reason)),
		message: format!("call failed: {reason}"),
		retryable: true,
		recovery_action: "retry_call".to_owned(),
	}
}

/// `CaptureInvalid` as `capture invalid`, for people to read.
fn words(camel: &str) -> String {
	screaming(camel).replace('_', " ").to_lowercase()
}

fn screaming(camel: &str) -> String {
	let mut out = String::with_capacity(camel.len() + 4);
	for (i, ch) in camel.chars().enumerate() {
		if ch.is_uppercase() && i > 0 {
			out.push('_');
		}
		out.push(ch.to_ascii_uppercase());
	}
	out
}

/// A peer, merging discovery data with the link state.
pub fn peer_to_pb(info: &PeerInfo, linked: bool) -> Peer {
	Peer {
		peer_id: info.id.as_bytes().to_vec(),
		display_name: info.display_name.clone(),
		local_addresses: info.addresses.clone(),
		trust: trust_to_pb(info.trust) as i32,
		reachable: info.reachable || linked,
		avatar_sha256: info.avatar.map(|h| h.to_vec()).unwrap_or_default(),
		shared_channel_ids: Vec::new(),
		port: info
			.addresses
			.iter()
			.find_map(|a| a.parse::<std::net::SocketAddr>().ok())
			.map_or(0, |a| u32::from(a.port())),
		last_seen_unix_ms: 0,
		favorite: false,
		favorite_position: 0,
		fingerprint_short: relay_net::identity::Fingerprint::from_bytes(*info.id.as_bytes())
			.short(),
		key_changed: info.key_change.is_some(),
		previous_peer_id: info
			.key_change
			.map(|previous| previous.as_bytes().to_vec())
			.unwrap_or_default(),
		previous_fingerprint_short: info
			.key_change
			.map(|previous| {
				relay_net::identity::Fingerprint::from_bytes(*previous.as_bytes()).short()
			})
			.unwrap_or_default(),
	}
}

/// A device.
pub fn device_to_pb(device: &DeviceInfo) -> Device {
	Device {
		device_id: device.id.clone(),
		name: device.name.clone(),
		direction: match device.direction {
			DeviceDirection::Input => PbDirection::Input,
			DeviceDirection::Output => PbDirection::Output,
		} as i32,
		available: device.available,
		system_default: device.system_default,
		supported_rates_hint: device.rates.clone(),
	}
}

/// Engine-wide settings.
pub fn settings_to_pb(global: &GlobalView, directory: &Directory) -> Settings {
	let aux = &directory.aux;
	let used = directory.device_use();
	Settings {
		input_is_default: used.input.is_default,
		output_is_default: used.output.is_default,
		input_device_name: used.input.name,
		output_device_name: used.output.name,
		input_pinned_missing: used.input.pinned_missing,
		output_pinned_missing: used.output.pinned_missing,
		revision: global.settings.get(),
		input_device_id: aux.input_device.clone(),
		output_device_id: aux.output_device.clone(),
		ns_backend_id: aux.noise_backend.clone(),
		global_muted: global.muted,
		global_deafened: global.deafened,
		microphone_gain: aux.microphone_gain,
		playback_gain: gain_to_f32(global.output_gain),
		display_name: aux.display_name.clone(),
		avatar_sha256: aux.avatar.map(|h| h.to_vec()).unwrap_or_default(),
		inbound_policy: inbound_to_pb(aux.inbound_policy) as i32,
		advert_mode: advert_to_pb(aux.advert_mode) as i32,
		autostart_daemon: aux.autostart_daemon,
		ringtone_enabled: aux.ring.ringtone_enabled,
		ringtone_volume: aux.ring.ringtone_volume,
		ringback_enabled: aux.ring.ringback_enabled,
		call_sounds_enabled: aux.ring.call_sounds_enabled,
		history_enabled: aux.history_enabled,
		history_revision: aux.history_revision,
		default_transmit_mode: mode_to_pb(aux.default_transmit_mode) as i32,
		effective_default_transmit_mode: mode_to_pb(aux.effective_default()) as i32,
		ptt_enabled: aux.ptt_enabled,
		daemon_build_revision: relay_api::BUILD_REVISION.to_owned(),
		daemon_api_revision: relay_api::API_REVISION,
		ptt_release_delay_ms: u32::try_from(global.ptt_release_delay.as_millis())
			.unwrap_or(u32::MAX),
	}
}

/// The full snapshot at API revision `revision`.
pub fn snapshot_to_pb(
	core: &CoreSnapshot,
	directory: &Directory,
	revision: u64,
	incarnation: &[u8; 16],
) -> Snapshot {
	Snapshot {
		engine_incarnation: incarnation.to_vec(),
		revision,
		peers: directory.peers(),
		calls: core
			.calls
			.values()
			.map(|v| call_to_pb(v, revision, directory))
			.collect(),
		devices: directory.devices(),
		settings: Some(settings_to_pb(&core.global, directory)),
		warnings: directory.warnings(),
		channels: directory.channels(),
	}
}

/// API events for one core event. Events with no client-visible effect
/// (attach, detach) yield nothing, so API revisions stay gapless.
pub fn events_of(event: &Event, revision: u64, directory: &mut Directory) -> Vec<StateEvent> {
	let single = |event| vec![StateEvent { event: Some(event) }];
	match event {
		Event::ClientAttached(_) | Event::ClientDetached(_) => Vec::new(),
		Event::LinkChanged { peer, up } => directory.link_changed(*peer, *up),
		Event::GlobalUpdated(view) => single(state_event::Event::Settings(settings_to_pb(
			view, directory,
		))),
		Event::CallUpdated(view) => single(state_event::Event::Call(call_to_pb(
			view, revision, directory,
		))),
		Event::CallForgotten(id) => single(state_event::Event::Removed(pb::Removed {
			id: id.to_bytes().to_vec(),
			kind: "call".to_owned(),
		})),
		Event::InputFault { call, fault } => single(state_event::Event::Warning(pb::Problem {
			code: format!("INPUT_FAULT_{}", screaming(&format!("{fault:?}"))),
			message: format!("push-to-talk authority withdrawn for call {call}"),
			retryable: true,
			recovery_action: "press_again".to_owned(),
		})),
	}
}
