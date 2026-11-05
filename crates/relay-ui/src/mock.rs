//! Mock-state mode: the UI runs on synthetic data, with no daemon. Commands
//! go through the same [`Controller`](crate::controller::Controller)
//! validation as live ones and mutate the hub directly.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use relay_client::Secret;

use crate::controller::SessionSlot;
use crate::engine::{Command, EngineError, Operations, Outcome};
use crate::hub::Hub;
use crate::model::{
	Advert, AppState, Call, CallMeter, Channel, ConnectionView, Device, Direction, EngineInfo,
	Inbound, Link, Meters, Mode, Peer, Phase, Settings, Transmit, Trust,
};

const MOCK_CLIENT: [u8; 16] = [7; 16];

fn id(n: u8, len: usize) -> String {
	hex::encode(vec![n; len])
}

fn call(n: u8, peer: u8, phase: Phase) -> Call {
	Call {
		call_id: id(n, 16),
		peer_id: id(peer, 32),
		phase,
		transmit: Transmit {
			mode: Mode::ReceiveOnly,
			consent_granted: false,
			effective_transmitting: false,
			blocked_reason: String::new(),
			releasing: false,
		},
		user_muted: false,
		user_deafened: false,
		receive_gain: 1.0,
		profile: "mock".into(),
		problem: None,
		controlled_by_me: true,
		controller_generation: 1,
	}
}

fn peer(n: u8, name: &str, trust: Trust, reachable: bool) -> Peer {
	Peer {
		peer_id: id(n, 32),
		display_name: name.into(),
		addresses: Vec::new(),
		trust,
		reachable,
		avatar_hash: String::new(),
		shared_channels: Vec::new(),
	}
}

fn device(name: &str, direction: Direction, default: bool) -> Device {
	Device {
		device_id: name.to_lowercase().replace(' ', "-"),
		name: name.into(),
		direction,
		available: true,
		system_default: default,
	}
}

/// Synthetic starting state.
pub fn seed() -> AppState {
	AppState {
		link: Link::Connected,
		mock: true,
		engine: Some(EngineInfo {
			version: "mock".into(),
			api_major: 1,
			api_minor: 0,
		}),
		revision: 1,
		peers: vec![
			peer(1, "Ada Lovelace", Trust::Verified, true),
			peer(2, "Grace Hopper", Trust::TofuUnverified, true),
			peer(3, "Alan Turing", Trust::KeyChanged, false),
		],
		calls: vec![call(100, 1, Phase::IncomingRinging)],
		devices: vec![
			device("Built-in Microphone", Direction::Input, true),
			device("USB Headset Mic", Direction::Input, false),
			device("Built-in Speakers", Direction::Output, true),
			device("USB Headset", Direction::Output, false),
		],
		settings: Some(Settings {
			input_device_id: "built-in-microphone".into(),
			output_device_id: "built-in-speakers".into(),
			global_muted: false,
			global_deafened: false,
			microphone_gain: 1.0,
			playback_gain: 1.0,
			ptt_release_delay_ms: 200,
			autostart_daemon: true,
			ringtone_enabled: true,
			ringtone_volume: 0.7,
			ringback_enabled: true,
			call_sounds_enabled: true,
			display_name: "Me".into(),
			avatar_hash: String::new(),
			inbound: Inbound::Anyone,
			advert: Advert::Visible,
		}),
		client_id: MOCK_CLIENT.to_vec(),
		..AppState::default()
	}
}

struct MockOps {
	hub: Arc<Hub>,
}

impl MockOps {
	fn with_call(&self, call_id: &[u8], f: impl FnOnce(&mut Call)) {
		let wanted = hex::encode(call_id);
		self.hub.update(|s| {
			if let Some(c) = s.calls.iter_mut().find(|c| c.call_id == wanted) {
				f(c);
			}
		});
	}
}

#[async_trait]
impl Operations for MockOps {
	async fn run(&self, command: Command) -> Result<Outcome, EngineError> {
		match command {
			Command::StartCall { peer_id } => self.hub.update(|s| {
				let n = s.calls.len() as u8 + 101;
				let mut c = call(n, 0, Phase::OutgoingRinging);
				c.peer_id = hex::encode(peer_id);
				s.calls.push(c);
			}),
			Command::AcquireController { .. } => {
				return Ok(Outcome {
					generation: Some(1),
					..Outcome::default()
				});
			}
			Command::AcceptCall { call_id, .. } => {
				self.with_call(&call_id, |c| c.phase = Phase::Connected)
			}
			Command::DeclineCall { call_id, .. } => {
				self.with_call(&call_id, |c| c.phase = Phase::Declined)
			}
			Command::EndCall { call_id, .. } => {
				self.with_call(&call_id, |c| c.phase = Phase::Ended)
			}
			Command::SetTransmit {
				call_id,
				mode,
				consent,
				..
			} => self.with_call(&call_id, |c| {
				// Mirrors the engine: nothing transmits for push-to-talk without a key.
				c.transmit = Transmit {
					mode,
					consent_granted: consent,
					effective_transmitting: consent && mode == Mode::OpenMic,
					blocked_reason: String::new(),
					releasing: false,
				};
			}),
			Command::SetCallControls {
				call_id,
				muted,
				deafened,
				receive_gain,
				..
			} => {
				self.with_call(&call_id, |c| {
					c.user_muted = muted.unwrap_or(c.user_muted);
					c.user_deafened = deafened.unwrap_or(c.user_deafened);
					c.receive_gain = receive_gain.unwrap_or(c.receive_gain);
				});
			}
			Command::SetGlobalControls {
				muted,
				deafened,
				microphone_gain,
				playback_gain,
				ptt_release_delay_ms,
			} => {
				self.hub.update(|s| {
					if let Some(g) = s.settings.as_mut() {
						g.global_muted = muted.unwrap_or(g.global_muted);
						g.global_deafened = deafened.unwrap_or(g.global_deafened);
						g.microphone_gain = microphone_gain.unwrap_or(g.microphone_gain);
						g.playback_gain = playback_gain.unwrap_or(g.playback_gain);
						g.ptt_release_delay_ms =
							ptt_release_delay_ms.unwrap_or(g.ptt_release_delay_ms);
					}
				});
			}
			Command::SetRingPreferences {
				ringtone_enabled,
				ringtone_volume,
				ringback_enabled,
				call_sounds_enabled,
			} => self.hub.update(|s| {
				if let Some(g) = s.settings.as_mut() {
					g.ringtone_enabled = ringtone_enabled.unwrap_or(g.ringtone_enabled);
					g.ringtone_volume = ringtone_volume.unwrap_or(g.ringtone_volume);
					g.ringback_enabled = ringback_enabled.unwrap_or(g.ringback_enabled);
					g.call_sounds_enabled = call_sounds_enabled.unwrap_or(g.call_sounds_enabled);
				}
			}),
			Command::PreviewTone => {}
			Command::SetAutostart { enabled } => self.hub.update(|s| {
				if let Some(g) = s.settings.as_mut() {
					g.autostart_daemon = enabled;
				}
			}),
			Command::SelectDevices {
				input_id,
				output_id,
			} => self.hub.update(|s| {
				if let Some(g) = s.settings.as_mut() {
					g.input_device_id = input_id;
					g.output_device_id = output_id;
				}
			}),
			Command::StartCallConnection { .. } => self.hub.update(|s| {
				let n = s.calls.len() as u8 + 101;
				s.calls.push(call(n, 1, Phase::OutgoingRinging));
			}),
			Command::CreateChannel { name, options, .. } => self.hub.update(|s| {
				s.channels.push(Channel {
					channel_id: id(200 + s.channels.len() as u8, 16),
					name,
					listed: options.listed,
					password: options.password,
					invite_only: options.invite_only,
					role: "owner".into(),
					shared_peers: 0,
					cert_expires_unix: 0,
				});
			}),
			Command::JoinChannel { channel_id, .. } => self.hub.update(|s| {
				s.channels.push(Channel {
					channel_id: hex::encode(channel_id),
					name: "joined".into(),
					listed: false,
					password: true,
					invite_only: false,
					role: "member".into(),
					shared_peers: 1,
					cert_expires_unix: 0,
				});
			}),
			Command::LeaveChannel { channel_id } => {
				let wanted = hex::encode(channel_id);
				self.hub
					.update(|s| s.channels.retain(|c| c.channel_id != wanted));
			}
			Command::CreateInvite { .. } => {
				return Ok(Outcome {
					invite: Some(Secret::new("RLYI1-MOCK-TOKEN-NOT-REAL")),
					..Outcome::default()
				});
			}
			Command::RevokeMember { .. } => {
				self.hub
					.update(|s| s.notice = Some("Mock: member revoked.".into()));
			}
			Command::SetPrivacy { inbound, advert } => self.hub.update(|s| {
				if let Some(g) = s.settings.as_mut() {
					g.inbound = inbound.unwrap_or(g.inbound);
					g.advert = advert.unwrap_or(g.advert);
				}
			}),
			Command::SetProfile { .. } => {
				self.hub
					.update(|s| s.notice = Some("Mock: profile name is not stored.".into()));
			}
			Command::UploadAvatar { .. } => {
				self.hub
					.update(|s| s.notice = Some("Mock: avatar is not stored.".into()));
			}
		}
		Ok(Outcome::default())
	}

	async fn connection_info(&self) -> Result<ConnectionView, EngineError> {
		Ok(ConnectionView {
			addresses: vec!["192.168.1.40".into()],
			port: 7000,
			fingerprint: MOCK_FINGERPRINT.into(),
			connection_string: format!("relay://192.168.1.40:7000#{MOCK_FINGERPRINT}"),
		})
	}

	async fn avatar(&self, _sha256: Vec<u8>) -> Result<Vec<u8>, EngineError> {
		Ok(crate::avatar::DEFAULT_PNG.to_vec())
	}
}

const MOCK_FINGERPRINT: &str = "ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12";

/// Synthetic meters for connected calls at time `t` seconds.
pub fn meters_at(state: &AppState, t: f32) -> Meters {
	let muted = state.settings.as_ref().is_some_and(|s| s.global_muted);
	Meters {
		seq: 0,
		microphone_dbfs: (!muted).then(|| -30.0 + 12.0 * t.sin()),
		calls: state
			.calls
			.iter()
			.filter(|c| c.phase == Phase::Connected)
			.enumerate()
			.map(|(i, c)| {
				let level = -28.0 + 14.0 * (t + i as f32).sin();
				CallMeter {
					call_id: c.call_id.clone(),
					speaking: level > -26.0,
					receive_dbfs: Some(level),
					rtt_ms: Some(4.0),
				}
			})
			.collect(),
	}
}

/// Seed the hub, accept commands, and feed meters until dropped.
pub async fn run(hub: Arc<Hub>, slot: Arc<SessionSlot>) {
	hub.update(|s| *s = seed());
	slot.set(Arc::new(MockOps { hub: hub.clone() }));
	let started = tokio::time::Instant::now();
	loop {
		tokio::time::sleep(Duration::from_millis(100)).await;
		if hub.visible() {
			let state = hub.state();
			if state.calls.iter().any(|c| c.phase == Phase::Connected) || state.settings.is_some() {
				hub.set_meters(meters_at(&state, started.elapsed().as_secs_f32() * 1.4));
			}
		}
	}
}
