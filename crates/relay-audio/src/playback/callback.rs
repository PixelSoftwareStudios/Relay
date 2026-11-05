//! The real-time half of playback. Nothing here allocates, locks, logs or does I/O.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::CHUNK_FRAMES;
use crate::convert::{Sample, fan_out};
use crate::ring::SampleConsumer;

/// Counters and flags shared with the control side.
#[derive(Debug, Default)]
pub struct PlaybackShared {
	muted: AtomicBool,
	flush_generation: AtomicU64,
	callbacks: AtomicU64,
	underrun_frames: AtomicU64,
	overflow_frames: AtomicU64,
}

impl PlaybackShared {
	/// Output silence while still consuming queued audio.
	pub fn set_muted(&self, muted: bool) {
		self.muted.store(muted, Ordering::Release);
	}

	/// Ask the callback to discard everything queued before this call.
	pub fn request_flush(&self) {
		self.flush_generation.fetch_add(1, Ordering::AcqRel);
	}

	pub fn callbacks(&self) -> u64 {
		self.callbacks.load(Ordering::Relaxed)
	}

	/// Frames filled with silence because no audio was queued.
	pub fn underrun_frames(&self) -> u64 {
		self.underrun_frames.load(Ordering::Relaxed)
	}

	/// Frames the writer dropped because the ring was full.
	pub fn overflow_frames(&self) -> u64 {
		self.overflow_frames.load(Ordering::Relaxed)
	}

	pub(super) fn add_overflow(&self, frames: u64) {
		self.overflow_frames.fetch_add(frames, Ordering::Relaxed);
	}
}

/// Playback callback state: owned by the audio thread.
pub struct PlaybackCallback {
	channels: usize,
	samples: SampleConsumer,
	shared: Arc<PlaybackShared>,
	seen_flush: u64,
}

impl PlaybackCallback {
	pub(super) fn new(channels: u16, samples: SampleConsumer, shared: Arc<PlaybackShared>) -> Self {
		Self {
			channels: usize::from(channels.max(1)),
			samples,
			shared,
			seen_flush: 0,
		}
	}

	/// Fill one interleaved device buffer. Underflow yields silence; the
	/// previous buffer is never replayed.
	pub fn process<S: Sample>(&mut self, out: &mut [S]) {
		self.shared.callbacks.fetch_add(1, Ordering::Relaxed);
		let flush = self.shared.flush_generation.load(Ordering::Acquire);
		if flush != self.seen_flush {
			self.seen_flush = flush;
			self.samples.discard_all();
		}
		let muted = self.shared.muted.load(Ordering::Acquire);

		let mut mono = [0.0_f32; CHUNK_FRAMES];
		for chunk in out.chunks_mut(CHUNK_FRAMES * self.channels) {
			let frames = chunk.len() / self.channels;
			let got = self.samples.pop(&mut mono[..frames]);
			if got < frames {
				mono[got..frames].fill(0.0);
				self.shared
					.underrun_frames
					.fetch_add((frames - got) as u64, Ordering::Relaxed);
			}
			if muted {
				mono[..frames].fill(0.0);
			}
			fan_out(&mono[..frames], self.channels, chunk);
		}
	}
}
