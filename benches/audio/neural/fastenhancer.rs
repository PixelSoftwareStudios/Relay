//! FastEnhancer (aask1357/fastenhancer), 48 kHz spec2spec ONNX from release
//! `onnx-48khz-v1`: 1024-point Hann STFT, per-size hop, GRU caches.
//!
//! The waveform-to-waveform exports (`fastenhancer_<size>.onnx`) do not run
//! in tract: it has no `SplitToSequence` operator. The spec2spec files do.

use std::path::Path;

use super::adapter::{Adapter, Info};
use super::model::{err, load_verified};
use super::spec::{SpecConfig, SpecModel, Window};
use crate::audio::Result;

pub const N_FFT: usize = 1024;

/// (hop, cache width, cache count) read from each export with
/// `audio-corpus model-info`.
fn layout(size: &str) -> Option<(usize, [usize; 2], usize)> {
	Some(match size {
		"t" => (512, [24, 20], 2),
		"s" => (512, [48, 48], 3),
		"b" => (512, [36, 36], 3),
		"m" => (320, [72, 72], 4),
		"l" => (200, [96, 96], 5),
		_ => return None,
	})
}

pub fn build(dir: &Path, size: &str) -> Result<Adapter> {
	let id: &'static str = Box::leak(format!("fastenhancer-{size}").into_boxed_str());
	let (hop, width, count) = layout(size).ok_or_else(|| err(id, "unknown size"))?;
	let file = format!("fastenhancer/fastenhancer_{size}_spec.onnx");
	let (blobs, sha256) = load_verified(dir, id, &[file.as_str()])?;
	let config = SpecConfig {
		id,
		n_fft: N_FFT,
		hop,
		window: Window::Hann,
		caches: (0..count).map(|_| vec![1, width[0], width[1]]).collect(),
	};
	Adapter::new(
		Box::new(SpecModel::new(config, &blobs[0])?),
		Info {
			id,
			version: "fastenhancer onnx-48khz-v1 spec2spec, tract-onnx 0.23".to_owned(),
			sha256,
			license: "MIT (repository LICENSE; training-data terms unreviewed)".to_owned(),
			rate: 48_000,
			window: N_FFT,
		},
	)
}
