//! The render side of the playback device: resampler, FIFO writer and the
//! clock-drift loop fed from the FIFO's sample-granular fill level.
//!
//! Two things keep latency honest. The fill target follows the device's
//! observed callback size (a callback larger than the fill would otherwise
//! be served short on every pull), and a fill that stays far above target,
//! as after a stall, is shed by dropping whole frames instead of waiting for
//! the 300 ppm drift loop to bleed it off over minutes.

use std::sync::Arc;

use relay_audio::convert::StreamResampler;
use relay_audio::error::Result;
use relay_audio::playback::{PlaybackShared, PlaybackWriter};
use relay_audio::{FRAME_SAMPLES, INTERNAL_RATE};
use relay_protocol::drift::{DriftConfig, DriftEstimator};

/// Ticks between drift observations (500 ms of output).
const DRIFT_PERIOD_TICKS: u32 = 50;
/// Consecutive ticks the fill may sit above the shed threshold before a
/// frame is dropped.
const SHED_AFTER_TICKS: u32 = 20;

/// Writes mixed 10 ms frames to the playback FIFO.
pub struct RenderSink {
	writer: PlaybackWriter,
	resampler: StreamResampler,
	drift: DriftEstimator,
	device_rate: u32,
	ticks: u32,
	/// FIFO fill just before the latest frame was written.
	before_write: usize,
	/// The configured floor of the target, in device samples.
	base_target: usize,
	/// The target currently steered toward, in device samples.
	target: usize,
	callbacks: Option<Arc<PlaybackShared>>,
	/// The callback size the backend expects, known before the first callback.
	callback_hint: usize,
	excess_ticks: u32,
	shed: u64,
}

impl RenderSink {
	/// `target_ms` is the FIFO fill the drift loop steers toward.
	pub fn new(writer: PlaybackWriter, device_rate: u32, target_ms: u32) -> Result<Self> {
		let target = (u64::from(device_rate) * u64::from(target_ms) + 500) / 1000;
		let target = usize::try_from(target).unwrap_or(usize::MAX);
		Ok(Self {
			writer,
			resampler: StreamResampler::new(INTERNAL_RATE, device_rate, FRAME_SAMPLES)?,
			drift: DriftEstimator::new(drift_config(device_rate), target_u32(target)),
			device_rate,
			ticks: 0,
			before_write: 0,
			base_target: target,
			target,
			callbacks: None,
			callback_hint: 0,
			excess_ticks: 0,
			shed: 0,
		})
	}

	/// Raises the fill target to fit the largest callback the device makes.
	#[must_use]
	pub fn track_callbacks(mut self, shared: Arc<PlaybackShared>) -> Self {
		self.callbacks = Some(shared);
		self
	}

	/// Plans for the callback size the backend announced at open, so the target
	/// fits it from the first write instead of after the first drift period.
	#[must_use]
	pub fn with_callback_hint(mut self, frames: Option<u32>) -> Self {
		self.callback_hint = frames.map_or(0, |f| f as usize);
		self.retarget();
		self
	}

	/// Queues silence up to the target fill. Call before the stream starts, so
	/// the device's first callbacks find data instead of underrunning while the
	/// worker is still waking up.
	pub fn prefill(&mut self) {
		let missing = self.target.saturating_sub(self.writer.occupancy());
		const CHUNK: [f32; 256] = [0.0; 256];
		let mut left = missing;
		while left > 0 {
			let n = left.min(CHUNK.len());
			let written = self.writer.write(&CHUNK[..n]);
			if written == 0 {
				break;
			}
			left -= written;
		}
	}

	/// Resamples one mixed frame into the FIFO and, periodically, updates
	/// the drift correction.
	pub fn write(&mut self, frame: &[f32]) -> Result<()> {
		self.before_write = self.writer.occupancy();
		self.ticks += 1;
		if self.ticks >= DRIFT_PERIOD_TICKS {
			self.ticks = 0;
			self.update_drift()?;
		}
		if self.should_shed() {
			// The FIFO has stayed deeper than it should be: skip this frame.
			// Remote audio loses 10 ms once; latency drops by the same amount.
			self.shed += 1;
			return Ok(());
		}
		let writer = &mut self.writer;
		self.resampler.push(frame, |block| {
			writer.write(block);
		})?;
		Ok(())
	}

	fn callback_frames(&self) -> usize {
		let seen = self
			.callbacks
			.as_ref()
			.map_or(0, |s| s.max_callback_frames() as usize);
		seen.max(self.callback_hint)
	}

	fn should_shed(&mut self) -> bool {
		let frame = self.device_rate as usize / 100;
		// The mean fill is half a frame above `before_write`; it may exceed the
		// target by half a callback of sawtooth plus one frame of margin.
		let threshold = self.target + self.callback_frames() / 2 + frame / 2;
		if self.before_write > threshold {
			self.excess_ticks += 1;
			if self.excess_ticks >= SHED_AFTER_TICKS {
				self.excess_ticks = SHED_AFTER_TICKS / 2;
				return true;
			}
		} else {
			self.excess_ticks = 0;
		}
		false
	}

	fn retarget(&mut self) {
		let callback = self.callback_frames();
		if callback == 0 {
			return;
		}
		let frame = self.device_rate as usize / 100;
		// Pre-pull fill must cover a whole callback: half a callback and half a
		// frame of sawtooth below the mean, plus the callback itself.
		let wanted = (callback * 3 / 2 + frame / 2).max(self.base_target);
		// Leave room in the FIFO for the sawtooth above the mean.
		let wanted = wanted.min(self.writer.capacity() * 3 / 4);
		if wanted != self.target {
			self.target = wanted;
			self.drift.set_target(target_u32(wanted));
		}
	}

	fn update_drift(&mut self) -> Result<()> {
		self.retarget();
		// The fill is a sawtooth: it peaks right after a frame is written and
		// is lowest right before. Half a frame above the minimum is the mean,
		// which is what the target describes.
		let mean = self.before_write + self.device_rate as usize / 200;
		let occupancy = u32::try_from(mean).unwrap_or(u32::MAX);
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

	/// The fill the loop currently steers toward, in device-rate samples.
	pub fn target(&self) -> usize {
		self.target
	}

	/// Frames dropped to bring the fill back to target.
	pub fn shed_frames(&self) -> u64 {
		self.shed
	}

	/// The drift correction currently applied, in ppm of the resampler ratio.
	pub fn drift_ppm(&self) -> f64 {
		-self.drift.snapshot().ratio_ppm
	}
}

fn target_u32(samples: usize) -> u32 {
	u32::try_from(samples).unwrap_or(u32::MAX)
}

/// The estimator's sample-count thresholds are 48 kHz numbers; scale them so
/// they mean the same number of milliseconds at the device rate. Gains are per
/// sample of error, so they scale the other way.
fn drift_config(device_rate: u32) -> DriftConfig {
	let scale = f64::from(device_rate) / 48_000.0;
	let base = DriftConfig::DEFAULT;
	DriftConfig {
		sample_rate: f64::from(device_rate),
		error_clamp: base.error_clamp * scale,
		transient_samples: base.transient_samples * scale,
		kp: base.kp / scale,
		ki: base.ki / scale,
		..base
	}
}
