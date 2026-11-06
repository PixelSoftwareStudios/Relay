//! Ring, ringback and blip playback of the media system.
//!
//! The render worker owns the [`TonePlayer`]; this side holds its only
//! [`ToneControl`]. Tones are mixed into the playback output after the call
//! gains. Nothing here touches capture, encoders or send gates, so a tone
//! cannot reach a peer. The playback stream stays open while a tone plays and
//! for a short hold afterwards, then closes if no call or test needs it.
//!
//! A ring never starts on a cold stream. The output is opened when an outgoing
//! call starts preparing (`Prewarm`) or when the tone is requested; the device
//! thread fills the FIFO with silence and gives the render worker its output
//! before the device is started, and the tone begins on the worker's first
//! tick with a 10 ms fade-in. A looping tone stays inside the player, so there
//! is no stream restart between ring cycles.

use relay_io::LockExt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use relay_audio::tones::{ToneControl, TonePlayer};

use super::devices::DeviceCmd;
use super::render_worker::RenderCmd;
use super::system::MediaSystem;
use crate::service::ToneCommand;

/// How long the output stays open after a one-shot starts (longer than any).
const ONE_SHOT_HOLD: Duration = Duration::from_millis(1500);
/// How long a warmed-up output waits for the ring that should follow.
const PREWARM_HOLD: Duration = Duration::from_secs(3);
/// How long the output stays open after a stop (the fade-out plus a margin).
const STOP_HOLD: Duration = Duration::from_millis(200);

/// The control side of the tone player.
pub(super) struct Tones {
	control: Mutex<ToneControl>,
	/// A tone needs the playback stream.
	pub(super) wanted: AtomicBool,
	/// Bumped by every tone command; a release only acts if it is still current.
	epoch: AtomicU64,
}

impl Tones {
	/// Creates the control side and the player to install in the render worker.
	pub(super) fn new() -> (Self, TonePlayer) {
		let (player, control) = TonePlayer::new();
		let tones = Self {
			control: Mutex::new(control),
			wanted: AtomicBool::new(false),
			epoch: AtomicU64::new(0),
		};
		(tones, player)
	}
}

impl MediaSystem {
	/// Applies a tone command, opening the output stream for a start.
	pub fn tone(&self, command: ToneCommand) {
		let tones = &self.inner.tones;
		let result = {
			let mut control = tones.control.lock_unpoisoned();
			match command {
				ToneCommand::Start {
					sound,
					looping,
					volume,
				} => control.start(sound, looping, volume),
				ToneCommand::Prewarm => Ok(()),
				ToneCommand::Stop => control.stop(),
				ToneCommand::Volume(volume) => control.set_volume(volume),
				ToneCommand::Muted(muted) => control.set_muted(muted),
			}
		};
		if let Err(error) = result {
			tracing::warn!(%error, "tone command refused");
			return;
		}
		match command {
			ToneCommand::Start { looping, .. } => {
				let epoch = tones.epoch.fetch_add(1, Ordering::AcqRel) + 1;
				tones.wanted.store(true, Ordering::Release);
				let system = self.clone();
				self.inner.runtime.spawn(async move {
					if let Err(error) = system.open_playback().await {
						tracing::warn!(%error, "output for a tone could not be opened");
					}
				});
				if !looping {
					self.release_later(epoch, ONE_SHOT_HOLD);
				}
			}
			ToneCommand::Prewarm => {
				let epoch = tones.epoch.fetch_add(1, Ordering::AcqRel) + 1;
				tones.wanted.store(true, Ordering::Release);
				let system = self.clone();
				self.inner.runtime.spawn(async move {
					if let Err(error) = system.open_playback().await {
						tracing::warn!(%error, "output could not be warmed up");
					}
				});
				self.release_later(epoch, PREWARM_HOLD);
			}
			ToneCommand::Stop => {
				let epoch = tones.epoch.fetch_add(1, Ordering::AcqRel) + 1;
				self.release_later(epoch, STOP_HOLD);
			}
			ToneCommand::Volume(_) | ToneCommand::Muted(_) => {}
		}
		self.inner.render.wake();
	}

	/// Whether a tone is keeping the playback stream open.
	pub(super) fn tone_wanted(&self) -> bool {
		self.inner.tones.wanted.load(Ordering::Acquire)
	}

	fn release_later(&self, epoch: u64, after: Duration) {
		let system = self.clone();
		self.inner.runtime.spawn(async move {
			tokio::time::sleep(after).await;
			system.release_tone_output(epoch);
		});
	}

	/// Lets go of the output unless a newer tone command arrived or a call or
	/// test still plays.
	fn release_tone_output(&self, epoch: u64) {
		let tones = &self.inner.tones;
		if tones.epoch.load(Ordering::Acquire) != epoch {
			return;
		}
		tones.wanted.store(false, Ordering::Release);
		let mut state = self.state();
		if state.calls.is_empty() && !self.inner.mic_test.wants_playback() {
			state.playback_open = false;
			drop(state);
			let _ = self.inner.devices.commands.send(DeviceCmd::ClosePlayback);
		}
	}

	pub(super) fn install_tone_player(&self, player: TonePlayer) {
		if self
			.inner
			.render
			.push(RenderCmd::SetTones(Box::new(player)))
			.is_err()
		{
			tracing::warn!("render queue full; tone player dropped");
		}
	}
}
