//! The restricted command layer: validates UI input and turns it into
//! engine [`Command`]s. It never decides engine truth; state only changes
//! through the engine's own events.

use std::sync::{Arc, Mutex};

use relay_io::LockExt;

use serde::{Serialize, Serializer};

use crate::engine::{Command, EngineError, Operations};
use crate::hub::Hub;
use crate::model::{Call, Mode};
use crate::tr;

pub const AVATAR_MAX_BYTES: usize = 1024 * 1024;
const NAME_MAX_CHARS: usize = 64;
const GAIN_MAX: f32 = 4.0;
const CALL_ID_LEN: usize = 16;
const PEER_ID_LEN: usize = 32;

/// A user-facing failure. Serializes as a plain string for the webview.
/// The text of `Invalid` and `NotConnected` is the UI's own and is worded in
/// the language active when the error was made; `Engine` text is the
/// engine's, shown as received.
#[derive(Debug, PartialEq)]
pub enum ControlError {
	NotConnected,
	Invalid(&'static str),
	Engine(String),
}

impl std::fmt::Display for ControlError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::NotConnected => f.write_str(tr!("err-not-connected")),
			Self::Invalid(text) => f.write_str(text),
			Self::Engine(text) => f.write_str(text),
		}
	}
}

impl std::error::Error for ControlError {}

impl From<EngineError> for ControlError {
	fn from(error: EngineError) -> Self {
		Self::Engine(error.to_string())
	}
}

impl Serialize for ControlError {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.collect_str(self)
	}
}

/// The live engine session, present only while connected.
#[derive(Clone)]
pub(crate) struct Session {
	pub(crate) ops: Arc<dyn Operations>,
}

/// Holder the supervisor fills on connect and empties on disconnect.
#[derive(Default)]
pub struct SessionSlot(Mutex<Option<Session>>);

impl SessionSlot {
	pub fn set(&self, ops: Arc<dyn Operations>) {
		*self.0.lock_unpoisoned() = Some(Session { ops });
	}

	pub fn clear(&self) {
		*self.0.lock_unpoisoned() = None;
	}

	pub fn is_connected(&self) -> bool {
		self.get().is_some()
	}

	pub(crate) fn get(&self) -> Option<Session> {
		self.0.lock_unpoisoned().clone()
	}
}

pub struct Controller {
	pub(crate) hub: Arc<Hub>,
	pub(crate) slot: Arc<SessionSlot>,
}

pub(crate) fn decode_id(
	hex_id: &str,
	len: usize,
	what: &'static str,
) -> Result<Vec<u8>, ControlError> {
	hex::decode(hex_id)
		.ok()
		.filter(|b| b.len() == len)
		.ok_or(ControlError::Invalid(what))
}

/// Longest push-to-talk release delay, in milliseconds.
pub const RELEASE_DELAY_MAX_MS: u32 = 2000;

fn check_release_delay(millis: u32) -> Result<(), ControlError> {
	if millis > RELEASE_DELAY_MAX_MS {
		return Err(ControlError::Invalid(tr!("err-release-delay")));
	}
	Ok(())
}

fn check_gain(gain: Option<f32>) -> Result<(), ControlError> {
	match gain {
		Some(g) if !g.is_finite() || !(0.0..=GAIN_MAX).contains(&g) => {
			Err(ControlError::Invalid(tr!("err-gain")))
		}
		_ => Ok(()),
	}
}

impl Controller {
	pub fn new(hub: Arc<Hub>, slot: Arc<SessionSlot>) -> Self {
		Self { hub, slot }
	}

	pub(crate) async fn run(
		&self,
		command: Command,
	) -> Result<crate::engine::Outcome, ControlError> {
		let session = self.slot.get().ok_or(ControlError::NotConnected)?;
		Ok(session.ops.run(command).await?)
	}

	fn call(&self, hex_id: &str) -> Result<(Vec<u8>, Call), ControlError> {
		let id = decode_id(hex_id, CALL_ID_LEN, tr!("err-bad-call-id"))?;
		let call = self
			.hub
			.read(|s| s.calls.iter().find(|c| c.call_id == hex_id).cloned())
			.ok_or(ControlError::Invalid(tr!("err-unknown-call")))?;
		Ok((id, call))
	}

	/// Generation of the call lease, acquiring control first when another
	/// client (or nobody) holds it. Never takes over silently.
	async fn generation(&self, call_id: &[u8], call: &Call) -> Result<u64, ControlError> {
		if call.controlled_by_me {
			return Ok(call.controller_generation);
		}
		let outcome = self
			.run(Command::AcquireController {
				call_id: call_id.to_vec(),
			})
			.await?;
		outcome
			.generation
			.ok_or(ControlError::Engine(tr!("err-no-lease").into()))
	}

	pub async fn start_call(&self, peer_id: &str) -> Result<(), ControlError> {
		let id = decode_id(peer_id, PEER_ID_LEN, tr!("err-bad-peer-id"))?;
		if !self
			.hub
			.read(|s| s.peers.iter().any(|p| p.peer_id == peer_id))
		{
			return Err(ControlError::Invalid(tr!("err-unknown-peer")));
		}
		self.run(Command::StartCall { peer_id: id }).await.map(drop)
	}

	pub async fn accept_call(&self, call_id: &str) -> Result<(), ControlError> {
		let (id, call) = self.call(call_id)?;
		let generation = self.generation(&id, &call).await?;
		self.run(Command::AcceptCall {
			call_id: id,
			generation,
		})
		.await
		.map(drop)
	}

	pub async fn decline_call(&self, call_id: &str) -> Result<(), ControlError> {
		let (id, call) = self.call(call_id)?;
		let generation = self.generation(&id, &call).await?;
		self.run(Command::DeclineCall {
			call_id: id,
			generation,
		})
		.await
		.map(drop)
	}

	pub async fn end_call(&self, call_id: &str) -> Result<(), ControlError> {
		let (id, call) = self.call(call_id)?;
		let generation = self.generation(&id, &call).await?;
		self.run(Command::EndCall {
			call_id: id,
			generation,
		})
		.await
		.map(drop)
	}

	/// Microphone transmission needs explicit consent in the same request.
	pub async fn set_transmit(
		&self,
		call_id: &str,
		mode: Mode,
		consent: bool,
	) -> Result<(), ControlError> {
		if mode != Mode::ReceiveOnly && !consent {
			return Err(ControlError::Invalid(tr!("err-consent-required")));
		}
		let (id, call) = self.call(call_id)?;
		let generation = self.generation(&id, &call).await?;
		let consent = consent && mode != Mode::ReceiveOnly;
		self.run(Command::SetTransmit {
			call_id: id,
			generation,
			mode,
			consent,
		})
		.await
		.map(drop)
	}

	pub async fn set_call_controls(
		&self,
		call_id: &str,
		muted: Option<bool>,
		deafened: Option<bool>,
		receive_gain: Option<f32>,
	) -> Result<(), ControlError> {
		check_gain(receive_gain)?;
		let (id, call) = self.call(call_id)?;
		let generation = self.generation(&id, &call).await?;
		self.run(Command::SetCallControls {
			call_id: id,
			generation,
			muted,
			deafened,
			receive_gain,
		})
		.await
		.map(drop)
	}

	pub async fn set_global_controls(
		&self,
		muted: Option<bool>,
		deafened: Option<bool>,
		microphone_gain: Option<f32>,
		playback_gain: Option<f32>,
	) -> Result<(), ControlError> {
		check_gain(microphone_gain)?;
		check_gain(playback_gain)?;
		self.run(Command::SetGlobalControls {
			muted,
			deafened,
			microphone_gain,
			playback_gain,
			ptt_release_delay_ms: None,
		})
		.await
		.map(drop)
	}

	/// Sets how long the gate stays open after the push-to-talk key is released.
	pub async fn set_ptt_release_delay(&self, millis: u32) -> Result<(), ControlError> {
		check_release_delay(millis)?;
		self.run(Command::SetGlobalControls {
			muted: None,
			deafened: None,
			microphone_gain: None,
			playback_gain: None,
			ptt_release_delay_ms: Some(millis),
		})
		.await
		.map(drop)
	}

	/// Sets the ring preferences; the daemon applies them to a playing ring at once.
	pub async fn set_ring_preferences(
		&self,
		ringtone_enabled: Option<bool>,
		ringtone_volume: Option<f32>,
		ringback_enabled: Option<bool>,
		call_sounds_enabled: Option<bool>,
	) -> Result<(), ControlError> {
		if ringtone_volume.is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v)) {
			return Err(ControlError::Invalid(tr!("err-ring-volume")));
		}
		self.run(Command::SetRingPreferences {
			ringtone_enabled,
			ringtone_volume,
			ringback_enabled,
			call_sounds_enabled,
		})
		.await
		.map(drop)
	}

	/// Plays the ring on this device's own output for about two seconds.
	pub async fn preview_tone(&self) -> Result<(), ControlError> {
		self.run(Command::PreviewTone).await.map(drop)
	}

	/// Whether this app starts the daemon by itself. The daemon stores it.
	pub async fn set_autostart(&self, enabled: bool) -> Result<(), ControlError> {
		self.run(Command::SetAutostart { enabled }).await.map(drop)
	}

	pub async fn select_devices(
		&self,
		input_id: &str,
		output_id: &str,
	) -> Result<(), ControlError> {
		let known = |id: &str| {
			self.hub
				.read(|s| s.devices.iter().any(|d| d.device_id == id && d.available))
		};
		if !known(input_id) || !known(output_id) {
			return Err(ControlError::Invalid(tr!("err-unknown-device")));
		}
		self.run(Command::SelectDevices {
			input_id: input_id.into(),
			output_id: output_id.into(),
		})
		.await
		.map(drop)
	}

	pub async fn set_profile(&self, display_name: &str) -> Result<(), ControlError> {
		let name = display_name.trim();
		if name.is_empty()
			|| name.chars().count() > NAME_MAX_CHARS
			|| name.chars().any(char::is_control)
		{
			return Err(ControlError::Invalid(tr!("err-display-name")));
		}
		self.run(Command::SetProfile {
			display_name: name.to_owned(),
		})
		.await
		.map(drop)
	}

	/// Remove the stored avatar (idempotent in the engine).
	pub async fn clear_avatar(&self) -> Result<(), ControlError> {
		self.run(Command::ClearAvatar).await.map(drop)
	}

	/// Raw encoded bytes only; the engine decodes and validates the image.
	pub async fn upload_avatar(
		&self,
		bytes: Vec<u8>,
		media_type: &str,
	) -> Result<(), ControlError> {
		if media_type.is_empty() {
			return Err(ControlError::Invalid(tr!("err-avatar-media-type")));
		}
		if bytes.is_empty() || bytes.len() > AVATAR_MAX_BYTES {
			return Err(ControlError::Invalid(crate::format::avatar_size_refusal()));
		}
		self.run(Command::UploadAvatar {
			bytes,
			media_type: media_type.to_owned(),
		})
		.await
		.map(drop)
	}
}
