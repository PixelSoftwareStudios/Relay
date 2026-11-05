//! The consumer half of capture; runs on a media worker.

use std::sync::Arc;

use rtrb::Consumer;

use super::{CaptureShared, GapEvent};
use crate::format::Discontinuity;
use crate::ring::SampleConsumer;

/// Result of one [`CaptureReader::read`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOutcome {
	/// Samples written to the output buffer.
	pub count: usize,
	/// Device-rate stream index of the first returned sample (or of the next
	/// sample to be read when `count` is zero).
	pub first_index: u64,
	/// Set when samples before `first_index` were lost or discarded.
	pub discontinuity: Option<Discontinuity>,
}

/// Reads device-rate mono samples with exact stream positions.
pub struct CaptureReader {
	samples: SampleConsumer,
	gaps: Consumer<GapEvent>,
	shared: Arc<CaptureShared>,
	popped: u64,
	index: u64,
	was_muted: bool,
}

impl CaptureReader {
	pub(super) fn new(
		samples: SampleConsumer,
		gaps: Consumer<GapEvent>,
		shared: Arc<CaptureShared>,
	) -> Self {
		Self {
			samples,
			gaps,
			shared,
			popped: 0,
			index: 0,
			was_muted: false,
		}
	}

	/// Samples currently queued.
	pub fn queued(&self) -> usize {
		self.samples.len()
	}

	/// Stream index of the next sample this reader will return.
	pub fn next_index(&self) -> u64 {
		self.index
	}

	/// Read up to `out.len()` samples. Never crosses a gap in one call.
	pub fn read(&mut self, out: &mut [f32]) -> ReadOutcome {
		let mut disc = None;
		if self.shared.is_muted() {
			self.drain(&mut disc);
			if !self.was_muted {
				disc = disc.or(Some(Discontinuity::Mute));
			}
			self.was_muted = true;
			return self.outcome(0, self.index, disc);
		}
		self.was_muted = false;

		// Count samples first so every visible sample's gap event is visible too.
		let available = self.samples.len();
		self.apply_events(&mut disc);
		let want = out.len().min(available).min(self.limit());
		let first = self.index;
		let n = self.samples.pop(&mut out[..want]);
		self.popped += n as u64;
		self.index += n as u64;
		self.outcome(n, first, disc)
	}

	fn outcome(&self, count: usize, first_index: u64, disc: Option<Discontinuity>) -> ReadOutcome {
		ReadOutcome {
			count,
			first_index,
			discontinuity: disc,
		}
	}

	/// Samples that can be popped before the next gap boundary.
	fn limit(&mut self) -> usize {
		match self.gaps.peek() {
			Ok(event) => (event.at_pushed - self.popped) as usize,
			Err(_) => usize::MAX,
		}
	}

	fn apply_events(&mut self, disc: &mut Option<Discontinuity>) {
		while let Ok(event) = self.gaps.peek().copied() {
			if event.at_pushed > self.popped {
				break;
			}
			self.index += event.skipped;
			*disc = Some(match (*disc, event.cause) {
				(Some(Discontinuity::Mute), _) | (_, Discontinuity::Mute) => Discontinuity::Mute,
				(_, cause) => cause,
			});
			let _ = self.gaps.pop();
		}
	}

	/// Throw away everything queued, keeping index accounting exact.
	fn drain(&mut self, disc: &mut Option<Discontinuity>) {
		loop {
			let available = self.samples.len();
			self.apply_events(disc);
			if available == 0 {
				break;
			}
			let count = available.min(self.limit());
			let n = self.samples.discard(count);
			self.popped += n as u64;
			self.index += n as u64;
			if n == 0 {
				break;
			}
		}
		self.apply_events(disc);
	}
}
