//! Turns a fixed-hop streaming model into a 10 ms / 48 kHz [`NoiseSuppressor`].
//!
//! 16 kHz models are fed through band-limited resamplers; models whose hop
//! is not a multiple of the 10 ms frame are bridged with FIFOs. The output
//! FIFO is primed with just enough zeros that it can never run dry, so the
//! adapter's own delay is the minimum that keeps the stream gapless. Nothing
//! here is reported as a delay: it is measured by impulse and correlation.

use std::collections::VecDeque;

use relay_audio::convert::StreamResampler;
use relay_audio::dsp::{
	DspDescriptor, DspError, DspLimits, NoiseSuppressor, ProcessProgress, ProcessStatus, Provides,
	StageKind, ThreadSafety,
};
use relay_audio::format::{BlockTiming, Discontinuity, FRAME_SAMPLES, INTERNAL_RATE};

use crate::audio::Result;

/// One streaming step of a model at its own sample rate.
pub trait HopModel: Send {
	/// Samples consumed (and produced) per step.
	fn hop(&self) -> usize;
	fn step(&mut self, input: &[f32], output: &mut [f32]) -> Result<()>;
	fn reset(&mut self) -> Result<()>;
}

pub struct Adapter {
	model: Box<dyn HopModel>,
	rate: u32,
	down: Option<StreamResampler>,
	up: Option<StreamResampler>,
	frame: usize,
	input: VecDeque<f32>,
	output: VecDeque<f32>,
	hop_in: Vec<f32>,
	hop_out: Vec<f32>,
	mid: Vec<f32>,
	wide: VecDeque<f32>,
	prime: usize,
	underruns: u64,
	descriptor: DspDescriptor,
}

fn gcd(a: usize, b: usize) -> usize {
	if b == 0 { a } else { gcd(b, a % b) }
}

pub struct Info {
	pub id: &'static str,
	pub version: String,
	pub sha256: String,
	pub license: String,
	pub rate: u32,
	pub window: usize,
}

impl Adapter {
	pub fn new(model: Box<dyn HopModel>, info: Info) -> Result<Self> {
		let bad = |e: relay_audio::AudioError| crate::audio::BenchError::Dsp(e.to_string());
		let hop = model.hop();
		let frame =
			(FRAME_SAMPLES as u64 * u64::from(info.rate) / u64::from(INTERNAL_RATE)) as usize;
		let (down, up) = if info.rate == INTERNAL_RATE {
			(None, None)
		} else {
			(
				Some(
					StreamResampler::new_fixed(INTERNAL_RATE, info.rate, FRAME_SAMPLES)
						.map_err(bad)?,
				),
				Some(StreamResampler::new_fixed(info.rate, INTERNAL_RATE, frame).map_err(bad)?),
			)
		};
		let prime = hop - gcd(hop, frame);
		let capacity = 4 * (hop + frame) + 1024;
		let mut output = VecDeque::with_capacity(capacity);
		output.extend(std::iter::repeat_n(0.0, prime));
		Ok(Self {
			rate: info.rate,
			down,
			up,
			frame,
			input: VecDeque::with_capacity(capacity),
			output,
			hop_in: vec![0.0; hop],
			hop_out: vec![0.0; hop],
			mid: vec![0.0; frame],
			wide: VecDeque::with_capacity(4 * FRAME_SAMPLES),
			prime,
			underruns: 0,
			descriptor: DspDescriptor {
				backend_id: info.id.to_owned(),
				version: info.version,
				model_sha256: Some(info.sha256),
				sample_rate_hz: info.rate,
				channels: 1,
				hop_samples: hop,
				analysis_window_samples: info.window,
				lookahead_samples: None,
				declared_delay_samples: None,
				warmup_samples: info.window,
				state_bytes: None,
				max_scratch_bytes: 0,
				thread_safety: ThreadSafety::SingleWorker,
				license_evidence: Some(info.license),
				provides: Provides::of(StageKind::NoiseSuppress),
			},
			model,
		})
	}

	/// Frames for which the output FIFO ran dry (zeros were substituted).
	pub fn underruns(&self) -> u64 {
		self.underruns
	}
}

fn backend(e: impl std::fmt::Display) -> DspError {
	DspError::Backend(e.to_string())
}

impl NoiseSuppressor for Adapter {
	fn descriptor(&self) -> &DspDescriptor {
		&self.descriptor
	}

	fn prepare(&mut self, _limits: &DspLimits) -> std::result::Result<(), DspError> {
		Ok(())
	}

	fn process_into(
		&mut self,
		input: &[f32],
		output: &mut [f32],
		timing: BlockTiming,
	) -> std::result::Result<ProcessProgress, DspError> {
		if input.len() < FRAME_SAMPLES {
			return Ok(ProcessProgress {
				consumed: 0,
				produced: 0,
				first_output_index: timing.first_sample,
				status: ProcessStatus::NeedMoreInput,
				discontinuity: None,
			});
		}
		if output.len() < FRAME_SAMPLES {
			return Err(DspError::OutputTooSmall {
				needed: FRAME_SAMPLES,
				available: output.len(),
			});
		}
		let input = &input[..FRAME_SAMPLES];
		match &mut self.down {
			Some(down) => {
				let fifo = &mut self.input;
				down.push(input, |b| fifo.extend(b.iter().map(|s| sane(*s))))
					.map_err(backend)?;
			}
			None => self.input.extend(input.iter().map(|s| sane(*s))),
		}
		let hop = self.hop_in.len();
		while self.input.len() >= hop {
			for (dst, src) in self.hop_in.iter_mut().zip(self.input.drain(..hop)) {
				*dst = src;
			}
			self.model
				.step(&self.hop_in, &mut self.hop_out)
				.map_err(backend)?;
			self.output.extend(self.hop_out.iter().copied());
		}
		for slot in &mut self.mid {
			*slot = self.output.pop_front().unwrap_or_else(|| {
				self.underruns += 1;
				0.0
			});
		}
		match &mut self.up {
			Some(up) => {
				let fifo = &mut self.wide;
				up.push(&self.mid, |b| fifo.extend(b.iter().copied()))
					.map_err(backend)?;
				// The resampler's first block is a few samples short (its
				// start-up delay); the gap is filled with leading silence.
				while self.wide.len() < FRAME_SAMPLES {
					self.wide.push_front(0.0);
					self.underruns += 1;
				}
				for slot in &mut output[..FRAME_SAMPLES] {
					*slot = self.wide.pop_front().unwrap_or(0.0);
				}
			}
			None => output[..FRAME_SAMPLES].copy_from_slice(&self.mid),
		}
		output[..FRAME_SAMPLES]
			.iter_mut()
			.for_each(|s| *s = s.clamp(-1.0, 1.0));
		Ok(ProcessProgress {
			consumed: FRAME_SAMPLES,
			produced: FRAME_SAMPLES,
			first_output_index: timing.first_sample,
			status: ProcessStatus::Ok,
			discontinuity: None,
		})
	}

	fn reset(&mut self, _reason: Discontinuity) {
		let _ = self.model.reset();
		if let Some(r) = &mut self.down {
			r.reset();
		}
		if let Some(r) = &mut self.up {
			r.reset();
		}
		self.input.clear();
		self.wide.clear();
		self.output.clear();
		self.output.extend(std::iter::repeat_n(0.0, self.prime));
	}
}

fn sane(s: f32) -> f32 {
	if s.is_finite() {
		s.clamp(-1.0, 1.0)
	} else {
		0.0
	}
}

impl Adapter {
	/// Model rate in Hz and samples per 10 ms frame at that rate.
	pub fn rates(&self) -> (u32, usize) {
		(self.rate, self.frame)
	}
}
