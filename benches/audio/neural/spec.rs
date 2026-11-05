//! Streaming spectrogram-to-spectrogram ONNX models: an STFT of the newest
//! `n_fft` samples goes in, an enhanced spectrum plus cache tensors come out,
//! and the inverse transform is overlap-added in Rust.
//!
//! Analysis and synthesis use the same window; the overlap-added output is
//! divided by the summed squared window (what `torch.istft` does), so any
//! hop works. Output samples become final `n_fft - hop` samples late, and
//! that delay is left in.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use tract_onnx::prelude::*;

use super::adapter::HopModel;
use super::model::{Plan, err, floats, plan, run, tensor};
use crate::audio::Result;

#[derive(Clone, Copy, Debug)]
pub enum Window {
	Hann,
	SqrtHann,
}

pub struct SpecConfig {
	pub id: &'static str,
	pub n_fft: usize,
	pub hop: usize,
	pub window: Window,
	/// Shapes of the cache tensors passed after the spectrum, in order.
	pub caches: Vec<Vec<usize>>,
}

pub struct SpecModel {
	id: &'static str,
	plan: Plan,
	n_fft: usize,
	hop: usize,
	bins: usize,
	forward: Arc<dyn RealToComplex<f32>>,
	inverse: Arc<dyn ComplexToReal<f32>>,
	window: Vec<f32>,
	/// Summed squared window for each of the `hop` samples finished per step.
	envelope: Vec<f32>,
	frame: Vec<f32>,
	windowed: Vec<f32>,
	time: Vec<f32>,
	ola: Vec<f32>,
	spec: Vec<Complex<f32>>,
	flat: Vec<f32>,
	scratch_f: Vec<Complex<f32>>,
	scratch_i: Vec<Complex<f32>>,
	shapes: Vec<Vec<usize>>,
	caches: Vec<Vec<f32>>,
}

/// Periodic window, as `torch.hann_window(n)` (optionally `.sqrt()`).
pub fn window(kind: Window, n: usize) -> Vec<f32> {
	(0..n)
		.map(|i| {
			let hann = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
			match kind {
				Window::Hann => hann as f32,
				Window::SqrtHann => hann.sqrt() as f32,
			}
		})
		.collect()
}

impl SpecModel {
	pub fn new(config: SpecConfig, onnx: &[u8]) -> Result<Self> {
		let SpecConfig {
			id,
			n_fft,
			hop,
			window: kind,
			caches,
		} = config;
		let mut planner = RealFftPlanner::<f32>::new();
		let forward = planner.plan_fft_forward(n_fft);
		let inverse = planner.plan_fft_inverse(n_fft);
		let bins = n_fft / 2 + 1;
		let window = window(kind, n_fft);
		let envelope = (0..hop)
			.map(|j| {
				(0..)
					.map(|k| j + k * hop)
					.take_while(|&i| i < n_fft)
					.map(|i| window[i] * window[i])
					.sum()
			})
			.collect();
		Ok(Self {
			id,
			plan: plan(id, onnx)?,
			n_fft,
			hop,
			bins,
			window,
			envelope,
			frame: vec![0.0; n_fft],
			windowed: vec![0.0; n_fft],
			time: vec![0.0; n_fft],
			ola: vec![0.0; n_fft],
			spec: vec![Complex::new(0.0, 0.0); bins],
			flat: vec![0.0; bins * 2],
			scratch_f: forward.make_scratch_vec(),
			scratch_i: inverse.make_scratch_vec(),
			caches: caches
				.iter()
				.map(|s| vec![0.0; s.iter().product()])
				.collect(),
			shapes: caches,
			forward,
			inverse,
		})
	}
}

impl HopModel for SpecModel {
	fn hop(&self) -> usize {
		self.hop
	}

	fn step(&mut self, input: &[f32], output: &mut [f32]) -> Result<()> {
		let (id, n, hop) = (self.id, self.n_fft, self.hop);
		self.frame.copy_within(hop.., 0);
		self.frame[n - hop..].copy_from_slice(input);
		for ((w, f), x) in self.windowed.iter_mut().zip(&self.frame).zip(&self.window) {
			*w = f * x;
		}
		self.forward
			.process_with_scratch(&mut self.windowed, &mut self.spec, &mut self.scratch_f)
			.map_err(|e| err(id, e))?;
		for (k, c) in self.spec.iter().enumerate() {
			self.flat[2 * k] = c.re;
			self.flat[2 * k + 1] = c.im;
		}
		let mut inputs: TVec<TValue> = tvec!(tensor(id, &[1, self.bins, 1, 2], &self.flat)?);
		for (shape, data) in self.shapes.iter().zip(&self.caches) {
			inputs.push(tensor(id, shape, data)?);
		}
		let out = run(id, &self.plan, inputs)?;
		let enhanced = floats(id, &out[0], self.bins * 2)?;
		for (k, c) in self.spec.iter_mut().enumerate() {
			*c = Complex::new(enhanced[2 * k], enhanced[2 * k + 1]);
		}
		for (i, cache) in self.caches.iter_mut().enumerate() {
			let len = cache.len();
			cache.copy_from_slice(floats(id, &out[i + 1], len)?);
		}
		self.spec[0].im = 0.0;
		self.spec[self.bins - 1].im = 0.0;
		self.inverse
			.process_with_scratch(&mut self.spec, &mut self.time, &mut self.scratch_i)
			.map_err(|e| err(id, e))?;
		let norm = 1.0 / n as f32;
		for ((o, t), w) in self.ola.iter_mut().zip(&self.time).zip(&self.window) {
			*o += t * w * norm;
		}
		for ((out, ola), env) in output.iter_mut().zip(&self.ola).zip(&self.envelope) {
			*out = if *env > 1e-8 { ola / env } else { 0.0 };
		}
		self.ola.copy_within(hop.., 0);
		self.ola[n - hop..].fill(0.0);
		Ok(())
	}

	fn reset(&mut self) -> Result<()> {
		self.frame.fill(0.0);
		self.ola.fill(0.0);
		self.caches.iter_mut().for_each(|c| c.fill(0.0));
		Ok(())
	}
}
