//! The real-time half of capture. Nothing here allocates, locks, logs or does I/O.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rtrb::Producer;

use super::{CHUNK_FRAMES, CaptureSetup, GapEvent};
use crate::convert::{ChannelSelect, Sample};
use crate::format::Discontinuity;
use crate::ring::SampleProducer;

/// Counters and flags shared with the control side. Lock-free only.
#[derive(Debug, Default)]
pub struct CaptureShared {
	muted: AtomicBool,
	callbacks: AtomicU64,
	frames_seen: AtomicU64,
	overrun_frames: AtomicU64,
}

impl CaptureShared {
	/// While muted the callback discards all input and the reader drops
	/// anything still queued, so old audio cannot reappear after unmute.
	pub fn set_muted(&self, muted: bool) {
		self.muted.store(muted, Ordering::Release);
	}

	pub fn is_muted(&self) -> bool {
		self.muted.load(Ordering::Acquire)
	}

	pub fn callbacks(&self) -> u64 {
		self.callbacks.load(Ordering::Relaxed)
	}

	/// Device frames observed, delivered or not.
	pub fn frames_seen(&self) -> u64 {
		self.frames_seen.load(Ordering::Relaxed)
	}

	/// Frames lost because the ring was full.
	pub fn overrun_frames(&self) -> u64 {
		self.overrun_frames.load(Ordering::Relaxed)
	}
}

/// Capture callback state: owned by the audio thread.
pub struct CaptureCallback {
	channels: usize,
	select: ChannelSelect,
	samples: SampleProducer,
	gaps: Producer<GapEvent>,
	shared: Arc<CaptureShared>,
	pushed: u64,
	pending_gap: u64,
	pending_cause: Discontinuity,
}

impl CaptureCallback {
	pub(super) fn new(
		setup: CaptureSetup,
		samples: SampleProducer,
		gaps: Producer<GapEvent>,
		shared: Arc<CaptureShared>,
	) -> Self {
		Self {
			channels: usize::from(setup.channels.max(1)),
			select: setup.select,
			samples,
			gaps,
			shared,
			pushed: 0,
			pending_gap: 0,
			pending_cause: Discontinuity::Overrun,
		}
	}

	/// Handle one interleaved device buffer.
	pub fn process<S: Sample>(&mut self, data: &[S]) {
		let frames = data.len() / self.channels;
		self.shared.callbacks.fetch_add(1, Ordering::Relaxed);
		self.shared
			.frames_seen
			.fetch_add(frames as u64, Ordering::Relaxed);

		if self.shared.is_muted() {
			self.skip(frames as u64, Discontinuity::Mute);
			return;
		}
		if !self.flush_gap() {
			self.skip(frames as u64, self.pending_cause);
			return;
		}

		let mut mono = [0.0_f32; CHUNK_FRAMES];
		let mut done = 0;
		while done < frames {
			let n = (frames - done).min(CHUNK_FRAMES);
			let chunk = &data[done * self.channels..(done + n) * self.channels];
			self.select.to_mono(chunk, self.channels, &mut mono);
			let written = self.samples.push(&mono[..n]);
			self.pushed += written as u64;
			done += written;
			if written < n {
				let lost = (frames - done) as u64;
				self.shared.overrun_frames.fetch_add(lost, Ordering::Relaxed);
				self.skip(lost, Discontinuity::Overrun);
				return;
			}
		}
	}

	fn skip(&mut self, frames: u64, cause: Discontinuity) {
		if self.pending_gap == 0 || cause == Discontinuity::Mute {
			self.pending_cause = cause;
		}
		self.pending_gap += frames;
	}

	/// Publish any pending gap; false if the queue is full (caller must keep skipping).
	fn flush_gap(&mut self) -> bool {
		if self.pending_gap == 0 {
			return true;
		}
		let event = GapEvent {
			at_pushed: self.pushed,
			skipped: self.pending_gap,
			cause: self.pending_cause,
		};
		if self.gaps.push(event).is_ok() {
			self.pending_gap = 0;
			true
		} else {
			false
		}
	}
}
