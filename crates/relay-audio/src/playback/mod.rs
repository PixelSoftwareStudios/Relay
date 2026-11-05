//! Playback path: the real-time callback and the writer feeding it.

mod callback;
mod writer;

pub use callback::{PlaybackCallback, PlaybackShared};
pub use writer::{PlaybackSink, PlaybackWriter};

use std::sync::Arc;

use crate::error::Result;
use crate::ring::sample_ring;

pub(crate) const CHUNK_FRAMES: usize = 256;

/// Build the connected callback and writer halves.
pub fn playback_pair(
	channels: u16,
	ring_samples: usize,
) -> (PlaybackCallback, PlaybackWriter, Arc<PlaybackShared>) {
	let shared = Arc::new(PlaybackShared::default());
	let (producer, consumer) = sample_ring(ring_samples);
	let callback = PlaybackCallback::new(channels, consumer, Arc::clone(&shared));
	let writer = PlaybackWriter::new(producer, Arc::clone(&shared));
	(callback, writer, shared)
}

/// Build a writer that resamples the internal domain to the device rate.
pub fn playback_sink(writer: PlaybackWriter, device_rate: u32) -> Result<PlaybackSink> {
	PlaybackSink::new(writer, device_rate)
}
