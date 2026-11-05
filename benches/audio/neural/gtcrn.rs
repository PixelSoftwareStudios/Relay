//! GTCRN (Xiaobin-Rong/gtcrn), streaming ONNX: 16 kHz, 512-point sqrt-Hann
//! STFT, hop 256, three cache tensors.

use std::path::Path;

use super::adapter::{Adapter, Info};
use super::model::load_verified;
use super::spec::{SpecConfig, SpecModel, Window};
use crate::audio::Result;

const ID: &str = "gtcrn";
const FILE: &str = "gtcrn/stream_onnx_models_gtcrn_simple.onnx";
pub const N_FFT: usize = 512;
pub const HOP: usize = 256;

pub fn config() -> SpecConfig {
	SpecConfig {
		id: ID,
		n_fft: N_FFT,
		hop: HOP,
		window: Window::SqrtHann,
		caches: vec![
			vec![2, 1, 16, 16, 33],
			vec![2, 3, 1, 1, 16],
			vec![2, 1, 33, 16],
		],
	}
}

pub fn build(dir: &Path) -> Result<Adapter> {
	let (blobs, sha256) = load_verified(dir, ID, &[FILE])?;
	Adapter::new(
		Box::new(SpecModel::new(config(), &blobs[0])?),
		Info {
			id: ID,
			version: "gtcrn stream/gtcrn_simple.onnx @502ebfa, tract-onnx 0.23".to_owned(),
			sha256,
			license: "MIT (repository LICENSE)".to_owned(),
			rate: 16_000,
			window: N_FFT,
		},
	)
}
