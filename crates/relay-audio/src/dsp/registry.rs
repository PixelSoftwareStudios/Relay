//! Backend selection. Neural backends stay stubbed until a qualified artifact exists.

use super::{DspError, ModelManifest, NoiseSuppressor, OffSuppressor, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
	Off,
	/// Candidate low-overhead backend (optional feature `rnnoise`).
	Rnnoise,
	/// Candidate stronger backend (optional feature `dtln`).
	Dtln,
}

impl BackendKind {
	fn name(self) -> &'static str {
		match self {
			Self::Off => "off",
			Self::Rnnoise => "rnnoise",
			Self::Dtln => "dtln",
		}
	}

	fn feature_enabled(self) -> bool {
		match self {
			Self::Off => true,
			Self::Rnnoise => cfg!(feature = "rnnoise"),
			Self::Dtln => cfg!(feature = "dtln"),
		}
	}
}

/// Construct a suppressor. Anything but `Off` needs its feature enabled and a
/// qualified, approved manifest; no model is vendored in this crate, and the
/// runtime integration does not exist yet, so those paths return clear errors.
pub fn create_suppressor(
	kind: BackendKind,
	manifest: Option<&ModelManifest>,
) -> Result<Box<dyn NoiseSuppressor>, DspError> {
	let unavailable = |reason: &str| DspError::BackendUnavailable {
		backend: kind.name().to_owned(),
		reason: reason.to_owned(),
	};
	if kind == BackendKind::Off {
		return Ok(Box::new(OffSuppressor::default()));
	}
	if !kind.feature_enabled() {
		return Err(unavailable("cargo feature is not enabled"));
	}
	let Some(manifest) = manifest else {
		return Err(unavailable("no qualification manifest supplied"));
	};
	if manifest.status != Status::Qualified || !manifest.distribution.approved {
		return Err(unavailable("manifest is not qualified and approved"));
	}
	manifest
		.validate()
		.map_err(|e| unavailable(&e.to_string()))?;
	Err(unavailable("runtime integration is not implemented; no vetted model artifact"))
}
