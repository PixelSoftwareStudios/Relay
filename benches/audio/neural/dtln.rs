//! DTLN (breizhn/DTLN), pretrained ONNX pair: 16 kHz, 512-sample block,
//! 128-sample shift, two stateful stages with an FFT mask in between.

use std::path::Path;
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use tract_onnx::prelude::*;

use super::adapter::{Adapter, HopModel, Info};
use super::model::{Plan, err, floats, load_verified, plan, run, tensor};
use crate::audio::Result;

const ID: &str = "dtln";
const FILES: [&str; 2] = [
	"dtln/pretrained_model_model_1.onnx",
	"dtln/pretrained_model_model_2.onnx",
];
pub const BLOCK: usize = 512;
pub const SHIFT: usize = 128;
const BINS: usize = BLOCK / 2 + 1;
const STATE: usize = 2 * 128 * 2;

pub struct Dtln {
	stage1: Plan,
	stage2: Plan,
	forward: Arc<dyn RealToComplex<f32>>,
	inverse: Arc<dyn ComplexToReal<f32>>,
	scratch_f: Vec<Complex<f32>>,
	scratch_i: Vec<Complex<f32>>,
	spec: Vec<Complex<f32>>,
	in_buffer: Vec<f32>,
	out_buffer: Vec<f32>,
	state1: Vec<f32>,
	state2: Vec<f32>,
}

impl Dtln {
	pub fn from_bytes(first: &[u8], second: &[u8]) -> Result<Self> {
		let mut planner = RealFftPlanner::<f32>::new();
		let forward = planner.plan_fft_forward(BLOCK);
		let inverse = planner.plan_fft_inverse(BLOCK);
		Ok(Self {
			stage1: plan("dtln model_1", first)?,
			stage2: plan("dtln model_2", second)?,
			scratch_f: forward.make_scratch_vec(),
			scratch_i: inverse.make_scratch_vec(),
			spec: vec![Complex::new(0.0, 0.0); BINS],
			in_buffer: vec![0.0; BLOCK],
			out_buffer: vec![0.0; BLOCK],
			state1: vec![0.0; STATE],
			state2: vec![0.0; STATE],
			forward,
			inverse,
		})
	}
}

impl HopModel for Dtln {
	fn hop(&self) -> usize {
		SHIFT
	}

	fn step(&mut self, input: &[f32], output: &mut [f32]) -> Result<()> {
		self.in_buffer.copy_within(SHIFT.., 0);
		self.in_buffer[BLOCK - SHIFT..].copy_from_slice(input);
		let mut frame = self.in_buffer.clone();
		self.forward
			.process_with_scratch(&mut frame, &mut self.spec, &mut self.scratch_f)
			.map_err(|e| err(ID, e))?;
		let mag: Vec<f32> = self.spec.iter().map(|c| c.norm()).collect();
		let out1 = run(
			ID,
			&self.stage1,
			tvec!(
				tensor(ID, &[1, 1, BINS], &mag)?,
				tensor(ID, &[1, 2, 128, 2], &self.state1)?
			),
		)?;
		let mask = floats(ID, &out1[0], BINS)?;
		self.state1.copy_from_slice(floats(ID, &out1[1], STATE)?);
		// magnitude * mask * exp(i * phase) == spectrum * mask
		for (c, m) in self.spec.iter_mut().zip(mask) {
			*c *= *m;
		}
		self.spec[0].im = 0.0;
		self.spec[BINS - 1].im = 0.0;
		let mut block = vec![0.0_f32; BLOCK];
		self.inverse
			.process_with_scratch(&mut self.spec, &mut block, &mut self.scratch_i)
			.map_err(|e| err(ID, e))?;
		let norm = 1.0 / BLOCK as f32;
		block.iter_mut().for_each(|s| *s *= norm);
		let out2 = run(
			ID,
			&self.stage2,
			tvec!(
				tensor(ID, &[1, 1, BLOCK], &block)?,
				tensor(ID, &[1, 2, 128, 2], &self.state2)?
			),
		)?;
		let estimated = floats(ID, &out2[0], BLOCK)?;
		self.state2.copy_from_slice(floats(ID, &out2[1], STATE)?);
		self.out_buffer.copy_within(SHIFT.., 0);
		self.out_buffer[BLOCK - SHIFT..].fill(0.0);
		for (o, e) in self.out_buffer.iter_mut().zip(estimated) {
			*o += e;
		}
		output.copy_from_slice(&self.out_buffer[..SHIFT]);
		Ok(())
	}

	fn reset(&mut self) -> Result<()> {
		self.in_buffer.fill(0.0);
		self.out_buffer.fill(0.0);
		self.state1.fill(0.0);
		self.state2.fill(0.0);
		Ok(())
	}
}

pub fn build(dir: &Path) -> Result<Adapter> {
	let (blobs, sha256) = load_verified(dir, ID, &FILES)?;
	Adapter::new(
		Box::new(Dtln::from_bytes(&blobs[0], &blobs[1])?),
		Info {
			id: "dtln",
			version: "dtln pretrained_model/model_{1,2}.onnx @1de1f15, tract-onnx 0.23".to_owned(),
			sha256,
			license: "MIT (repository LICENSE)".to_owned(),
			rate: 16_000,
			window: BLOCK,
		},
	)
}
