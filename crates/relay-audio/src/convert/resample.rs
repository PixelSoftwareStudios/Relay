//! Streaming mono rate conversion with sample-index accounting.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
	Adjustable, Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
	WindowFunction,
};

use crate::error::{AudioError, Result};

/// Conservative clamp on drift correction until device evidence widens it.
pub const MAX_DRIFT_PPM: f64 = 300.0;

/// Headroom the resampler is built with, above the clamp.
const MAX_RELATIVE_RATIO: f64 = 1.001;

/// Band-limited fixed-chunk mono resampler.
///
/// Equal rates bypass conversion entirely. Input and output positions are
/// counted so callers can keep sample indices across the conversion.
pub struct StreamResampler {
	core: Option<Core>,
	in_rate: u32,
	out_rate: u32,
	consumed: u64,
	produced: u64,
}

struct Core {
	inner: Async<f32>,
	pending: Vec<f32>,
	pending_len: usize,
	output: Vec<f32>,
	base_ratio: f64,
}

impl StreamResampler {
	/// Build a converter taking `chunk_in` input samples per processed chunk.
	/// Allocates; call during setup only.
	pub fn new(in_rate: u32, out_rate: u32, chunk_in: usize) -> Result<Self> {
		if in_rate == 0 || out_rate == 0 || chunk_in == 0 {
			return Err(AudioError::InvalidArgument("rates and chunk must be nonzero"));
		}
		let core = if in_rate == out_rate {
			None
		} else {
			let ratio = f64::from(out_rate) / f64::from(in_rate);
			let params = SincInterpolationParameters {
				sinc_len: 128,
				f_cutoff: None,
				oversampling_factor: 128,
				interpolation: SincInterpolationType::Linear,
				window: WindowFunction::BlackmanHarris2,
			};
			let inner = Async::<f32>::new_sinc(
				ratio,
				MAX_RELATIVE_RATIO,
				&params,
				chunk_in,
				1,
				FixedAsync::Input,
			)
			.map_err(|e| AudioError::Resample(e.to_string()))?;
			let output = vec![0.0; inner.output_frames_max()];
			Some(Core {
				pending: vec![0.0; inner.input_frames_max().max(chunk_in)],
				pending_len: 0,
				output,
				inner,
				base_ratio: ratio,
			})
		};
		Ok(Self {
			core,
			in_rate,
			out_rate,
			consumed: 0,
			produced: 0,
		})
	}

	pub fn in_rate(&self) -> u32 {
		self.in_rate
	}

	pub fn out_rate(&self) -> u32 {
		self.out_rate
	}

	/// Input samples accepted since construction or the last reset.
	pub fn consumed(&self) -> u64 {
		self.consumed
	}

	/// Output samples emitted since construction or the last reset.
	pub fn produced(&self) -> u64 {
		self.produced
	}

	/// Group delay of the conversion in output samples.
	pub fn delay_samples(&self) -> usize {
		self.core.as_ref().map_or(0, |c| c.inner.output_delay())
	}

	/// Apply a relative drift correction, clamped to +/- [`MAX_DRIFT_PPM`].
	/// A bypassed converter ignores it.
	pub fn set_drift_ppm(&mut self, ppm: f64) -> Result<()> {
		if let Some(core) = &mut self.core {
			let ppm = ppm.clamp(-MAX_DRIFT_PPM, MAX_DRIFT_PPM);
			core.inner
				.set_resample_ratio(core.base_ratio * (1.0 + ppm * 1e-6), false)
				.map_err(|e| AudioError::Resample(e.to_string()))?;
		}
		Ok(())
	}

	/// Drop buffered state, restarting the position counters.
	pub fn reset(&mut self) {
		if let Some(core) = &mut self.core {
			core.inner.reset();
			core.pending_len = 0;
		}
		self.consumed = 0;
		self.produced = 0;
	}

	/// Feed samples; `sink` receives each converted block. No allocation.
	pub fn push(&mut self, mut input: &[f32], mut sink: impl FnMut(&[f32])) -> Result<()> {
		self.consumed += input.len() as u64;
		let Some(core) = &mut self.core else {
			self.produced += input.len() as u64;
			sink(input);
			return Ok(());
		};
		while !input.is_empty() {
			let need = core.inner.input_frames_next();
			let take = (need - core.pending_len).min(input.len());
			core.pending[core.pending_len..core.pending_len + take].copy_from_slice(&input[..take]);
			core.pending_len += take;
			input = &input[take..];
			if core.pending_len < need {
				break;
			}
			let frames_out = core.inner.output_frames_next();
			let adapter_in = InterleavedSlice::new(&core.pending[..need], 1, need)
				.map_err(|e| AudioError::Resample(e.to_string()))?;
			let mut adapter_out = InterleavedSlice::new_mut(&mut core.output[..], 1, frames_out)
				.map_err(|e| AudioError::Resample(e.to_string()))?;
			let (_, written) = core
				.inner
				.process_into_buffer(&adapter_in, &mut adapter_out, None)
				.map_err(|e| AudioError::Resample(e.to_string()))?;
			core.pending_len = 0;
			self.produced += written as u64;
			sink(&core.output[..written]);
		}
		Ok(())
	}
}
