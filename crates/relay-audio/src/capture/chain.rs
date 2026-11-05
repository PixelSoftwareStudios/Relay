//! Reader, rate conversion and frame assembly for one capture stream.

use super::{CaptureReader, FrameAssembler};
use crate::convert::StreamResampler;
use crate::error::Result;
use crate::format::{BlockTiming, Discontinuity, INTERNAL_RATE};

const READ_CHUNK: usize = 1024;

/// What one [`CaptureChain::poll`] observed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PollStats {
	pub frames_emitted: usize,
	pub device_samples: usize,
	pub discontinuity: Option<Discontinuity>,
}

/// Turns device-rate mono samples into timed 480-sample frames.
pub struct CaptureChain {
	reader: CaptureReader,
	resampler: StreamResampler,
	assembler: FrameAssembler,
	scratch: Vec<f32>,
	device_rate: u32,
	started: bool,
}

impl CaptureChain {
	/// Allocates every buffer; call during setup.
	pub fn new(reader: CaptureReader, device_rate: u32) -> Result<Self> {
		let chunk = (device_rate / 100).max(1) as usize;
		Ok(Self {
			reader,
			resampler: StreamResampler::new(device_rate, INTERNAL_RATE, chunk)?,
			assembler: FrameAssembler::default(),
			scratch: vec![0.0; READ_CHUNK],
			device_rate,
			started: false,
		})
	}

	/// Conversion group delay in internal-domain samples. Frame timing is
	/// not shifted by it; the ledger accounts for it separately.
	pub fn delay_samples(&self) -> usize {
		self.resampler.delay_samples()
	}

	pub fn reader(&self) -> &CaptureReader {
		&self.reader
	}

	/// Drain what the device has delivered, handing each frame to `on_frame`.
	/// Work per call is bounded by the ring capacity. No allocation.
	pub fn poll(&mut self, mut on_frame: impl FnMut(&[f32], BlockTiming)) -> Result<PollStats> {
		let mut stats = PollStats::default();
		loop {
			let outcome = self.reader.read(&mut self.scratch);
			if !self.started && outcome.discontinuity.is_none() && outcome.count > 0 {
				self.restart(outcome.first_index);
				self.started = true;
			}
			if let Some(cause) = outcome.discontinuity {
				stats.discontinuity = Some(cause);
				self.restart(outcome.first_index);
				self.started = true;
			}
			if outcome.count == 0 {
				break;
			}
			stats.device_samples += outcome.count;
			let assembler = &mut self.assembler;
			self.resampler
				.push(&self.scratch[..outcome.count], |block| {
					assembler.push(block, |frame, timing| {
						stats.frames_emitted += 1;
						on_frame(frame, timing);
					});
				})?;
		}
		Ok(stats)
	}

	fn restart(&mut self, device_index: u64) {
		self.resampler.reset();
		let base =
			u128::from(device_index) * u128::from(INTERNAL_RATE) / u128::from(self.device_rate);
		self.assembler.restart(base as u64);
	}
}
