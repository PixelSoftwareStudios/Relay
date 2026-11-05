//! Pure-Rust ONNX noise-suppression candidates (feature `neural`): GTCRN,
//! DTLN and FastEnhancer, run with `tract` on one thread.
//!
//! Weights live in `benches/models/<family>/` (untracked, untrusted); their
//! hashes are checked against the committed manifests in
//! `benches/models/manifests/` before a model is built.

pub mod adapter;
pub mod dtln;
pub mod fastenhancer;
pub mod gtcrn;
pub mod info;
pub mod model;
pub mod spec;

use std::path::{Path, PathBuf};

use relay_audio::dsp::NoiseSuppressor;

use crate::audio::{BenchError, Result};
/// Backend names accepted by [`make`].
pub const NAMES: [&str; 7] = [
	"gtcrn",
	"dtln",
	"fastenhancer-t",
	"fastenhancer-s",
	"fastenhancer-b",
	"fastenhancer-m",
	"fastenhancer-l",
];

pub fn models_dir() -> PathBuf {
	std::env::var_os("RELAY_MODELS_DIR").map_or_else(
		|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models"),
		PathBuf::from,
	)
}

pub fn is_neural(name: &str) -> bool {
	NAMES.contains(&name)
}

/// Build a candidate by name. Fails (with the reason) if the weights are
/// missing or do not match the committed manifest.
pub fn make(name: &str) -> Result<Box<dyn NoiseSuppressor>> {
	make_in(name, &models_dir())
}

pub fn make_in(name: &str, dir: &Path) -> Result<Box<dyn NoiseSuppressor>> {
	let adapter = match name {
		"gtcrn" => gtcrn::build(dir)?,
		"dtln" => dtln::build(dir)?,
		other => match other.strip_prefix("fastenhancer-") {
			Some(size @ ("t" | "s" | "b" | "m" | "l")) => fastenhancer::build(dir, size)?,
			_ => return Err(BenchError::BackendUnavailable(other.to_owned())),
		},
	};
	Ok(Box::new(adapter))
}
