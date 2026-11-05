//! The render side of the playback device: resampler, FIFO writer and the
//! clock-drift loop fed from the FIFO's sample-granular fill level.

use relay_audio::convert::StreamResampler;
use relay_audio::error::Result;
use relay_audio::playback::PlaybackWriter;
use relay_audio::{FRAME_SAMPLES, INTERNAL_RATE};
use relay_protocol::drift::{DriftConfig, DriftEstimator};

/// Ticks between drift observations (500 ms of output).
const DRIFT_PERIOD_TICKS: u32 = 50;

/// Writes mixed 10 ms frames to the playback FIFO.
pub struct RenderSink {
	writer: PlaybackWriter,
	resampler: StreamResampler,
	drift: DriftEstimator,
	device_rate: u32,
	ticks: u32,
}

impl RenderSink {
	/// `target_ms` is the FIFO fill the drift loop steers toward.
	pub fn new(writer: PlaybackWriter, device_rate: u32, target_ms: u32) -> Result<Self> {
		let target = device_rate / 1000 * target_ms;
		let config = DriftConfig {
			sample_rate: f64::from(device_rate),
			..DriftConfig::DEFAULT
		};
		Ok(Self {
			writer,
			resampler: StreamResampler::new(INTERNAL_RATE, device_rate, FRAME_SAMPLES)?,
			drift: DriftEstimator::new(config, target),
			device_rate,
			ticks: 0,
		})
	}

	/// Resamples one mixed frame into the FIFO and, periodically, updates
	/// the drift correction.
	pub fn write(&mut self, frame: &[f32]) -> Result<()> {
		let writer = &mut self.writer;
		self.resampler.push(frame, |block| {
			writer.write(block);
		})?;
		self.ticks += 1;
		if self.ticks >= DRIFT_PERIOD_TICKS {
			self.ticks = 0;
			self.update_drift()?;
		}
		Ok(())
	}

	fn update_drift(&mut self) -> Result<()> {
		let occupancy = u32::try_from(self.writer.occupancy()).unwrap_or(u32::MAX);
		let elapsed = self.device_rate / 100 * DRIFT_PERIOD_TICKS;
		let ppm = self.drift.update(occupancy, elapsed);
		// The estimator reports "consume input faster" when the buffer is too
		// full. The FIFO is the output of the resampler, so a too-full FIFO
		// needs fewer output samples per input sample: the opposite sign.
		self.resampler.set_drift_ppm(-ppm)
	}

	/// FIFO fill in device-rate samples.
	pub fn occupancy(&self) -> usize {
		self.writer.occupancy()
	}

	/// The drift correction currently applied, in ppm of the resampler ratio.
	pub fn drift_ppm(&self) -> f64 {
		-self.drift.snapshot().ratio_ppm
	}
}
