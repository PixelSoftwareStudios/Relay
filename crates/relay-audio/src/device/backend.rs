//! The narrow backend boundary: enumerate, open capture/render, stop.

use super::{DeviceHandle, Enumeration, SampleFormatKind, StreamHealth};
use crate::capture::{CaptureReader, CaptureShared};
use crate::convert::ChannelSelect;
use crate::error::Result;
use crate::playback::{PlaybackShared, PlaybackWriter};
use std::sync::Arc;

/// Device buffer requested for call, tone and capture streams, in frames at the
/// internal rate (about 10.7 ms at 48 kHz). The default buffer of a desktop
/// device is often 1024 frames or more, which a 20 ms playout FIFO cannot
/// cover; a small fixed buffer keeps the callback size known and the latency
/// low. Backends clamp it to the device's supported range and fall back to the
/// device default when the range is unknown or the stream refuses it.
pub const LOW_LATENCY_BUFFER_FRAMES: u32 = 512;

/// Requested stream shape. The backend reports what it actually opened.
#[derive(Debug, Clone, Copy)]
pub struct StreamRequest {
	pub sample_rate: u32,
	/// Playback channel count request; capture uses the device's channels.
	pub channels: Option<u16>,
	pub select: ChannelSelect,
	pub buffer_frames: Option<u32>,
}

impl Default for StreamRequest {
	fn default() -> Self {
		Self {
			sample_rate: crate::format::INTERNAL_RATE,
			channels: None,
			select: ChannelSelect::default(),
			buffer_frames: None,
		}
	}
}

/// Configuration the backend actually opened. A requested buffer size is not
/// measured latency; `buffer_frames` is `None` when the backend cannot say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActualConfig {
	pub sample_rate: u32,
	pub channels: u16,
	pub format: SampleFormatKind,
	pub buffer_frames: Option<u32>,
	/// The callback size the device is expected to use, when the backend can
	/// say before the first callback (a fixed buffer was granted, or the
	/// native burst). The playout FIFO is sized from it before play starts.
	pub callback_frames: Option<u32>,
	pub max_callback_frames: u32,
}

/// Lifecycle control for an opened stream. Dropping it stops the stream.
pub trait StreamHandle {
	fn play(&self) -> Result<()>;
	fn pause(&self) -> Result<()>;
}

pub struct CaptureStream {
	pub config: ActualConfig,
	pub reader: CaptureReader,
	pub shared: Arc<CaptureShared>,
	pub health: StreamHealth,
	pub handle: Box<dyn StreamHandle>,
}

pub struct PlaybackStream {
	pub config: ActualConfig,
	pub writer: PlaybackWriter,
	pub shared: Arc<PlaybackShared>,
	pub health: StreamHealth,
	pub handle: Box<dyn StreamHandle>,
}

pub trait AudioBackend {
	/// Scan devices; handles from earlier scans become invalid.
	fn enumerate(&mut self) -> Result<Enumeration>;
	fn open_capture(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> Result<CaptureStream>;
	fn open_playback(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> Result<PlaybackStream>;
}

/// Ring capacity in mono samples: at least 40 ms, and always room for two
/// maximum-size callbacks plus 10 ms of scheduling slack. Fixed at open;
/// never resized inside a callback.
pub fn ring_samples_for(rate: u32, max_callback_frames: u32) -> usize {
	let forty_ms = rate as usize * 40 / 1000;
	let needed = max_callback_frames as usize * 2 + rate as usize / 100;
	forty_ms.max(needed)
}
