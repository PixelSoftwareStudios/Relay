//! Ring, ringback and blip playback for the daemon.
//!
//! [`TonePlayer`] owns the decoded sounds and is driven from the real-time
//! side; [`ToneControl`] is the single-producer control handle. Rendering
//! (`render_into`) allocates nothing, takes no locks and never logs. Tones are
//! mixed after per-call gains through [`crate::mixer::Mixer::add_tones`], so
//! master gain and the limiter apply, and they are part of the playback
//! signal (hence of the AEC render reference). They never reach capture or an
//! encoder.

mod assets;

use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
use thiserror::Error;

/// Fade length for start, stop and mute changes (10 ms).
pub const FADE_SAMPLES: usize = 480;
/// Pending control commands the player can hold.
pub const COMMAND_CAPACITY: usize = 32;
/// Custom rings that can wait for the player (and be handed back to the control side).
const RING_SWAPS: usize = 4;

/// An embedded sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
	/// Incoming-call ring (a soft two-note chime); loop period 4.0 s.
	Ring = 0,
	/// Outgoing-call ringback (two slow swells); loop period 4.0 s.
	Ringback = 1,
	/// Falling one-shot blip.
	CallEnd = 2,
	/// Rising one-shot blip.
	Connected = 3,
}

/// Control-side failures.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ToneError {
	#[error("tone volume must be finite and within 0.0..=1.0")]
	InvalidVolume,
	#[error("tone command queue is full")]
	QueueFull,
}

/// What the player is doing, as seen by the render side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToneState {
	Idle,
	Playing {
		sound: Sound,
		looping: bool,
		gain: f32,
		position: usize,
	},
}

#[derive(Debug, Clone, Copy)]
enum Command {
	Start {
		sound: Sound,
		looping: bool,
		gain: f32,
	},
	Stop,
	SetGain(f32),
	SetMuted(bool),
}

/// Control handle; keep exactly one (it is the queue's only producer).
pub struct ToneControl {
	tx: Producer<Command>,
	ring_tx: Producer<Option<Arc<[f32]>>>,
	retired_rx: Consumer<Arc<[f32]>>,
}

impl ToneControl {
	/// Start `sound`. Starting what already plays only updates the volume;
	/// starting a different sound fades the current one out first.
	pub fn start(&mut self, sound: Sound, looping: bool, volume: f32) -> Result<(), ToneError> {
		let gain = checked_volume(volume)?;
		self.send(Command::Start {
			sound,
			looping,
			gain,
		})
	}

	/// Fade out and go idle; a no-op when idle.
	pub fn stop(&mut self) -> Result<(), ToneError> {
		self.send(Command::Stop)
	}

	/// Change the playing volume (ramped).
	pub fn set_volume(&mut self, volume: f32) -> Result<(), ToneError> {
		self.send(Command::SetGain(checked_volume(volume)?))
	}

	/// Gate for global deafen: while muted the output is silent (ramped) and
	/// the sound keeps its position.
	pub fn set_muted(&mut self, muted: bool) -> Result<(), ToneError> {
		self.send(Command::SetMuted(muted))
	}

	/// Replace the incoming-call ring with `ring` (48 kHz mono, already faded and padded with
	/// its pause; its length is the loop period) or restore the built-in chime with `None`.
	/// The previous buffer is handed back here, never freed on the real-time side.
	pub fn set_custom_ring(&mut self, ring: Option<Arc<[f32]>>) -> Result<(), ToneError> {
		while self.retired_rx.pop().is_ok() {}
		self.ring_tx.push(ring).map_err(|_| ToneError::QueueFull)
	}

	fn send(&mut self, command: Command) -> Result<(), ToneError> {
		self.tx.push(command).map_err(|_| ToneError::QueueFull)
	}
}

fn checked_volume(volume: f32) -> Result<f32, ToneError> {
	if volume.is_finite() && (0.0..=1.0).contains(&volume) {
		Ok(volume)
	} else {
		Err(ToneError::InvalidVolume)
	}
}

#[derive(Debug, Clone, Copy)]
struct Active {
	sound: Sound,
	looping: bool,
	position: usize,
	stopping: bool,
}

/// Plays one tone at a time into the mix.
pub struct TonePlayer {
	rx: Consumer<Command>,
	ring_rx: Consumer<Option<Arc<[f32]>>>,
	retired_tx: Producer<Arc<[f32]>>,
	custom: Option<Arc<[f32]>>,
	sounds: [Vec<f32>; 4],
	active: Option<Active>,
	pending: Option<(Sound, bool, f32)>,
	env: f32,
	gain: f32,
	gain_target: f32,
	muted: bool,
	mute_env: f32,
}

impl TonePlayer {
	/// Decode every sound once and create the control handle.
	pub fn new() -> (Self, ToneControl) {
		let (tx, rx) = RingBuffer::new(COMMAND_CAPACITY);
		let (ring_tx, ring_rx) = RingBuffer::new(RING_SWAPS);
		let (retired_tx, retired_rx) = RingBuffer::new(RING_SWAPS + 2);
		let sounds = Sound::ALL.map(|s| assets::decode(s.bytes()));
		let player = Self {
			rx,
			ring_rx,
			retired_tx,
			custom: None,
			sounds,
			active: None,
			pending: None,
			env: 0.0,
			gain: 0.0,
			gain_target: 0.0,
			muted: false,
			mute_env: 1.0,
		};
		(
			player,
			ToneControl {
				tx,
				ring_tx,
				retired_rx,
			},
		)
	}

	pub fn state(&self) -> ToneState {
		match self.active {
			Some(a) => ToneState::Playing {
				sound: a.sound,
				looping: a.looping,
				gain: self.gain_target,
				position: a.position,
			},
			None => ToneState::Idle,
		}
	}

	/// Applies queued control commands without rendering, so an idle owner can
	/// tell whether a tone was just started.
	pub fn poll_commands(&mut self) {
		self.take_rings();
		while let Ok(command) = self.rx.pop() {
			self.apply(command);
		}
	}

	/// Installs the newest custom ring; the old buffer goes back to the control side so no
	/// memory is freed here (if that queue were ever full it is leaked rather than freed).
	fn take_rings(&mut self) {
		while let Ok(next) = self.ring_rx.pop() {
			if let Some(old) = std::mem::replace(&mut self.custom, next)
				&& let Err(rtrb::PushError::Full(old)) = self.retired_tx.push(old)
			{
				std::mem::forget(old);
			}
		}
	}

	/// Samples of `sound` and its loop period (the custom ring replaces the built-in one).
	fn data(&self, sound: Sound) -> (&[f32], usize) {
		match (&self.custom, sound) {
			(Some(ring), Sound::Ring) => (ring, ring.len()),
			_ => (&self.sounds[sound.index()], sound.period_samples()),
		}
	}

	/// True when nothing is playing or queued to play.
	pub fn is_idle(&self) -> bool {
		self.active.is_none() && self.pending.is_none()
	}

	/// Drain commands and ADD the tone to `out` (mono, 48 kHz). Safe to call
	/// from an audio callback.
	pub fn render_into(&mut self, out: &mut [f32]) {
		self.poll_commands();
		let step = 1.0 / FADE_SAMPLES as f32;
		for dst in out.iter_mut() {
			if self.active.is_none() {
				match self.pending.take() {
					Some((sound, looping, gain)) => self.begin(sound, looping, gain),
					None => break,
				}
			}
			let Some(mut a) = self.active else { break };
			let target = if a.stopping { 0.0 } else { 1.0 };
			self.env = approach(self.env, target, step);
			self.gain = approach(self.gain, self.gain_target, step);
			let mute_target = if self.muted { 0.0 } else { 1.0 };
			self.mute_env = approach(self.mute_env, mute_target, step);

			let (data, period) = self.data(a.sound);
			let sample = data.get(a.position).copied().unwrap_or(0.0);
			*dst += sample * self.gain * self.env * self.mute_env;

			a.position += 1;
			let mut done = a.stopping && self.env <= 0.0;
			if a.position >= period {
				if a.looping {
					a.position = 0;
				} else {
					done = true;
				}
			}
			self.active = if done { None } else { Some(a) };
		}
	}

	fn apply(&mut self, command: Command) {
		match command {
			Command::Start {
				sound,
				looping,
				gain,
			} => match &mut self.active {
				Some(a) if a.sound == sound && a.looping == looping && !a.stopping => {
					self.gain_target = gain;
				}
				Some(a) => {
					a.stopping = true;
					self.pending = Some((sound, looping, gain));
				}
				None => self.pending = Some((sound, looping, gain)),
			},
			Command::Stop => {
				self.pending = None;
				if let Some(a) = &mut self.active {
					a.stopping = true;
				}
			}
			Command::SetGain(gain) => {
				self.gain_target = gain;
				if let Some((_, _, g)) = &mut self.pending {
					*g = gain;
				}
			}
			Command::SetMuted(muted) => self.muted = muted,
		}
	}

	fn begin(&mut self, sound: Sound, looping: bool, gain: f32) {
		self.active = Some(Active {
			sound,
			looping,
			position: 0,
			stopping: false,
		});
		self.env = 0.0;
		self.gain = gain;
		self.gain_target = gain;
	}
}

fn approach(value: f32, target: f32, step: f32) -> f32 {
	if value < target {
		(value + step).min(target)
	} else {
		(value - step).max(target)
	}
}
