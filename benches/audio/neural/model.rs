//! Shared helpers: manifest-checked file loading, tract plans, tensors.

use std::path::Path;

use relay_audio::dsp::ModelManifest;
use sha2::{Digest, Sha256};
use tract_onnx::prelude::*;

use crate::audio::{BenchError, Result};

pub type Plan = Arc<TypedRunnableModel>;

pub fn err(context: &str, e: impl std::fmt::Display) -> BenchError {
	BenchError::Dsp(format!("{context}: {e}"))
}

/// Read the committed manifest for `id`.
pub fn manifest(dir: &Path, id: &str) -> Result<ModelManifest> {
	let path = dir.join("manifests").join(format!("{id}.json"));
	let text = std::fs::read_to_string(&path).map_err(|e| err(&path.display().to_string(), e))?;
	ModelManifest::from_json(&text).map_err(|e| err(&path.display().to_string(), e))
}

/// Read weight files and check the SHA-256 of their concatenation against
/// the manifest. Returns the bytes of each file.
pub fn load_verified(dir: &Path, id: &str, files: &[&str]) -> Result<(Vec<Vec<u8>>, String)> {
	let manifest = manifest(dir, id)?;
	let expected = manifest
		.model
		.sha256
		.clone()
		.ok_or_else(|| err(id, "manifest has no sha256"))?;
	let mut hasher = Sha256::new();
	let mut blobs = Vec::new();
	for file in files {
		let path = dir.join(file);
		let bytes = std::fs::read(&path).map_err(|e| {
			err(
				&path.display().to_string(),
				format!("{e} (download the weights named in manifests/{id}.json)"),
			)
		})?;
		hasher.update(&bytes);
		blobs.push(bytes);
	}
	let actual = hex::encode(hasher.finalize());
	if actual != expected {
		return Err(err(
			id,
			format!("weights hash {actual} does not match the manifest {expected}"),
		));
	}
	Ok((blobs, actual))
}

/// Parse, optimise and make an ONNX model runnable (input shapes are fixed
/// in these exports).
pub fn plan(id: &str, bytes: &[u8]) -> Result<Plan> {
	tract_onnx::onnx()
		.model_for_read(&mut std::io::Cursor::new(bytes))
		.and_then(|m| m.into_optimized())
		.and_then(|m| m.into_runnable())
		.map_err(|e| err(&format!("{id}: tract"), format!("{e:#}")))
}

pub fn tensor(id: &str, shape: &[usize], data: &[f32]) -> Result<TValue> {
	Tensor::from_shape(shape, data)
		.map(Into::into)
		.map_err(|e| err(id, e))
}

pub fn run(id: &str, plan: &Plan, inputs: TVec<TValue>) -> Result<TVec<TValue>> {
	plan.run(inputs).map_err(|e| err(id, format!("{e:#}")))
}

pub fn floats<'a>(id: &str, value: &'a TValue, expect: usize) -> Result<&'a [f32]> {
	let slice = value
		.try_as_plain_ram()
		.and_then(|v| v.as_slice::<f32>())
		.map_err(|e| err(id, e))?;
	if slice.len() != expect {
		return Err(err(
			id,
			format!("output holds {} values, expected {expect}", slice.len()),
		));
	}
	Ok(slice)
}
