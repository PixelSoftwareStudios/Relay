//! Decides which tone the daemon plays and tells the media port.
//!
//! [`ToneDriver`] is pure: it turns the call list, the presence flag, the
//! deafen state and the ring preferences into [`ToneCommand`]s, remembering only
//! what it last asked for. Tones only reach the playback mix; nothing here has
//! a path to capture, encoders or send gates.

use std::collections::BTreeMap;
use std::time::Duration;

use relay_audio::tones::Sound;
use relay_core::{CallId, CallState, Direction};
use tokio::time::Instant;

/// Ringback volume; it is not a preference.
pub const RINGBACK_VOLUME: f32 = 0.7;
/// How long the settings "test ring" plays.
pub const PREVIEW_LENGTH: Duration = Duration::from_secs(2);

const VOLUME_EPSILON: f32 = 1e-4;

/// One instruction for the tone player on the media side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToneCommand {
	/// Start `sound` (a sound that already plays only changes volume).
	Start {
		/// What to play.
		sound: Sound,
		/// Repeat until stopped.
		looping: bool,
		/// Volume, `0.0..=1.0`.
		volume: f32,
	},
	/// Fade out whatever plays.
	Stop,
	/// Change the volume of what plays.
	Volume(f32),
	/// Global deafen gate.
	Muted(bool),
}

/// Ring preferences.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingPrefs {
	/// Ring for an incoming call while a client is attached.
	pub ringtone_enabled: bool,
	/// Ring volume, `0.0..=1.0`.
	pub ringtone_volume: f32,
	/// Ringback while an outgoing call rings.
	pub ringback_enabled: bool,
	/// Connect and hang-up blips.
	pub call_sounds_enabled: bool,
}

impl Default for RingPrefs {
	fn default() -> Self {
		Self {
			ringtone_enabled: true,
			ringtone_volume: 0.7,
			ringback_enabled: true,
			call_sounds_enabled: true,
		}
	}
}

/// A partial change of the ring preferences.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RingUpdate {
	/// New ring switch.
	pub ringtone_enabled: Option<bool>,
	/// New ring volume.
	pub ringtone_volume: Option<f32>,
	/// New ringback switch.
	pub ringback_enabled: Option<bool>,
	/// New blip switch.
	pub call_sounds_enabled: Option<bool>,
}

impl RingUpdate {
	/// Nothing is changing.
	pub fn is_empty(&self) -> bool {
		*self == Self::default()
	}

	/// `prefs` with this change applied.
	pub fn apply(&self, prefs: RingPrefs) -> RingPrefs {
		RingPrefs {
			ringtone_enabled: self.ringtone_enabled.unwrap_or(prefs.ringtone_enabled),
			ringtone_volume: self.ringtone_volume.unwrap_or(prefs.ringtone_volume),
			ringback_enabled: self.ringback_enabled.unwrap_or(prefs.ringback_enabled),
			call_sounds_enabled: self
				.call_sounds_enabled
				.unwrap_or(prefs.call_sounds_enabled),
		}
	}
}

/// What the driver looks at.
pub struct ToneInput<'a> {
	/// Every known call with its direction and state.
	pub calls: &'a [(CallId, Direction, CallState)],
	/// At least one client is attached.
	pub attached: bool,
	/// Global deafen.
	pub deafened: bool,
	/// Preferences.
	pub prefs: RingPrefs,
	/// Now.
	pub now: Instant,
}

#[derive(Debug, Clone, Copy)]
struct Playing {
	sound: Sound,
	looping: bool,
	volume: f32,
}

/// Remembers the tone last requested and derives the next commands.
#[derive(Debug, Default)]
pub struct ToneDriver {
	playing: Option<Playing>,
	muted: bool,
	/// Calls that reached `Active`, so their end is announced once.
	connected: BTreeMap<CallId, bool>,
	preview_until: Option<Instant>,
}

impl ToneDriver {
	/// Starts the settings "test ring"; it yields to any real tone.
	pub fn preview(&mut self, now: Instant) {
		self.preview_until = Some(now + PREVIEW_LENGTH);
	}

	/// When the driver next needs a look even if nothing else changes.
	pub fn deadline(&self) -> Option<Instant> {
		self.preview_until
	}

	/// Whether the next [`sync`](Self::sync) may differ from the last one only
	/// because time passed.
	pub fn timed(&self) -> bool {
		self.preview_until.is_some()
	}

	/// Commands that make the player match `input`.
	pub fn sync(&mut self, input: &ToneInput<'_>) -> Vec<ToneCommand> {
		let mut out = Vec::new();
		let blip = self.blip(input);
		if input.deafened != self.muted {
			self.muted = input.deafened;
			out.push(ToneCommand::Muted(input.deafened));
		}
		if input.deafened {
			self.preview_until = None;
			return self.stop(out);
		}
		let looped = self.wanted(input);
		match (looped, self.playing) {
			(Some((sound, volume)), Some(now)) if now.looping && now.sound == sound => {
				if (now.volume - volume).abs() > VOLUME_EPSILON {
					out.push(ToneCommand::Volume(volume));
					self.playing = Some(Playing { volume, ..now });
				}
			}
			(Some((sound, volume)), _) => self.start(&mut out, sound, true, volume),
			(None, Some(now)) if now.looping => return self.stop(out),
			(None, _) => {
				if let Some(sound) = blip {
					self.start(&mut out, sound, false, input.prefs.ringtone_volume);
				}
			}
		}
		out
	}

	/// Commands for an orderly stop at shutdown.
	pub fn shutdown(&mut self) -> Vec<ToneCommand> {
		self.preview_until = None;
		self.stop(Vec::new())
	}

	fn start(&mut self, out: &mut Vec<ToneCommand>, sound: Sound, looping: bool, volume: f32) {
		out.push(ToneCommand::Start {
			sound,
			looping,
			volume,
		});
		self.playing = Some(Playing {
			sound,
			looping,
			volume,
		});
	}

	fn stop(&mut self, mut out: Vec<ToneCommand>) -> Vec<ToneCommand> {
		if self.playing.take().is_some() {
			out.push(ToneCommand::Stop);
		}
		out
	}

	/// The looping tone that should play now, if any.
	fn wanted(&mut self, input: &ToneInput<'_>) -> Option<(Sound, f32)> {
		let prefs = input.prefs;
		let ringing = |direction, state| {
			input
				.calls
				.iter()
				.any(|(_, d, s)| *d == direction && *s == state)
		};
		if prefs.ringtone_enabled
			&& input.attached
			&& ringing(Direction::Incoming, CallState::IncomingRinging)
		{
			return Some((Sound::Ring, prefs.ringtone_volume));
		}
		if prefs.ringback_enabled && ringing(Direction::Outgoing, CallState::OutgoingRinging) {
			return Some((Sound::Ringback, RINGBACK_VOLUME));
		}
		match self.preview_until {
			Some(until) if input.now < until => Some((Sound::Ring, prefs.ringtone_volume)),
			_ => {
				self.preview_until = None;
				None
			}
		}
	}

	/// The blip a call transition calls for, if the preferences allow one.
	fn blip(&mut self, input: &ToneInput<'_>) -> Option<Sound> {
		self.connected
			.retain(|id, _| input.calls.iter().any(|(call, ..)| call == id));
		let mut blip = None;
		for (id, _, state) in input.calls {
			let connected = self.connected.entry(*id).or_insert(false);
			if *state == CallState::Active && !*connected {
				*connected = true;
				blip = blip.or(Some(Sound::Connected));
			} else if state.is_terminal() && *connected {
				*connected = false;
				blip = Some(Sound::CallEnd);
			}
		}
		blip.filter(|_| input.prefs.call_sounds_enabled)
	}
}
