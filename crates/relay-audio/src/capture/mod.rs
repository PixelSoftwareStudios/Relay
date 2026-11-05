//! Capture path: real-time callback side, reader side and the processing chain.

mod assembler;
mod callback;
mod chain;
mod reader;

pub use assembler::FrameAssembler;
pub use callback::{CaptureCallback, CaptureShared};
pub use chain::{CaptureChain, PollStats};
pub use reader::{CaptureReader, ReadOutcome};

use std::sync::Arc;

use rtrb::RingBuffer;

use crate::convert::ChannelSelect;
use crate::format::Discontinuity;
use crate::ring::sample_ring;

/// Frames converted per stack-buffer pass inside the callback.
pub(crate) const CHUNK_FRAMES: usize = 256;
/// Capacity of the gap-event queue between callback and reader.
const GAP_QUEUE: usize = 64;

/// A span of source samples that never reached the ring.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GapEvent {
	/// Samples pushed before the gap began.
	pub at_pushed: u64,
	/// Source samples skipped.
	pub skipped: u64,
	pub cause: Discontinuity,
}

/// Static description of a capture stream's shape.
#[derive(Debug, Clone, Copy)]
pub struct CaptureSetup {
	pub channels: u16,
	pub select: ChannelSelect,
	/// Ring capacity in mono samples at the device rate.
	pub ring_samples: usize,
}

/// Build the connected callback and reader halves.
pub fn capture_pair(setup: CaptureSetup) -> (CaptureCallback, CaptureReader, Arc<CaptureShared>) {
	let shared = Arc::new(CaptureShared::default());
	let (producer, consumer) = sample_ring(setup.ring_samples);
	let (gap_tx, gap_rx) = RingBuffer::new(GAP_QUEUE);
	let callback = CaptureCallback::new(setup, producer, gap_tx, Arc::clone(&shared));
	let reader = CaptureReader::new(consumer, gap_rx, Arc::clone(&shared));
	(callback, reader, shared)
}
