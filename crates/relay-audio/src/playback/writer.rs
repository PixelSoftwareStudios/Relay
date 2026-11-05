//! Control-side writers feeding the playback ring.

use std::sync::Arc;

use super::PlaybackShared;
use crate::convert::StreamResampler;
use crate::error::Result;
use crate::format::INTERNAL_RATE;
use crate::ring::SampleProducer;

/// Writes device-rate mono samples; drops (and counts) what does not fit.
pub struct PlaybackWriter {
	producer: SampleProducer,
	shared: Arc<PlaybackShared>,
}

impl PlaybackWriter {
	pub(super) fn new(producer: SampleProducer, shared: Arc<PlaybackShared>) -> Self {
		Self { producer, shared }
	}

	pub fn write(&mut self, samples: &[f32]) -> usize {
		let written = self.producer.push(samples);
		if written < samples.len() {
			self.shared.add_overflow((samples.len() - written) as u64);
		}
		written
	}

	/// Free space in device-rate samples.
	pub fn vacant(&self) -> usize {
		self.producer.vacant()
	}

	/// Samples currently queued.
	pub fn occupancy(&self) -> usize {
		self.producer.capacity() - self.producer.vacant()
	}
}

/// 48 kHz writer with bounded drift correction from ring occupancy.
pub struct PlaybackSink {
	writer: PlaybackWriter,
	resampler: StreamResampler,
	target: f64,
	filtered: f64,
}

impl PlaybackSink {
	pub(super) fn new(writer: PlaybackWriter, device_rate: u32) -> Result<Self> {
		let chunk = (INTERNAL_RATE / 100) as usize;
		let target = f64::from(device_rate) * 0.02;
		Ok(Self {
			writer,
			resampler: StreamResampler::new(INTERNAL_RATE, device_rate, chunk)?,
			target,
			filtered: target,
		})
	}

	/// Queue internal-domain samples for the device.
	pub fn write(&mut self, samples: &[f32]) -> Result<()> {
		let writer = &mut self.writer;
		self.resampler.push(samples, |block| {
			writer.write(block);
		})
	}

	/// Slow proportional drift controller: nudges the ratio against the
	/// filtered ring occupancy, within the resampler's clamp. This is a
	/// starting point, not a validated controller.
	pub fn update_drift(&mut self) -> Result<()> {
		let occupancy = self.writer.occupancy() as f64;
		self.filtered += (occupancy - self.filtered) * 0.02;
		self.resampler.set_drift_ppm(-(self.filtered - self.target))
	}

	pub fn writer(&self) -> &PlaybackWriter {
		&self.writer
	}

	pub fn delay_samples(&self) -> usize {
		self.resampler.delay_samples()
	}
}
