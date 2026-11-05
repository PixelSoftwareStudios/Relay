//! Model qualification manifest (schema version 1) with schema-equivalent validation.

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
	Candidate,
	Qualified,
	Rejected,
}

/// Fields the schema requires but allows to be null: a missing key is an
/// error, an explicit `null` is `None`.
fn required<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
	D: Deserializer<'de>,
	T: Deserialize<'de>,
{
	Option::<T>::deserialize(deserializer)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
	pub url: String,
	#[serde(deserialize_with = "required")]
	pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
	pub variant: String,
	#[serde(deserialize_with = "required")]
	pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Runtime {
	pub name: String,
	#[serde(deserialize_with = "required")]
	pub version: Option<String>,
	pub threads: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioSpec {
	#[serde(deserialize_with = "required")]
	pub sample_rate_hz: Option<u32>,
	#[serde(deserialize_with = "required")]
	pub hop_samples: Option<u32>,
	#[serde(deserialize_with = "required")]
	pub analysis_window_samples: Option<u32>,
	#[serde(deserialize_with = "required")]
	pub measured_total_delay_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
	#[serde(deserialize_with = "required")]
	pub code_license: Option<String>,
	#[serde(deserialize_with = "required")]
	pub model_license: Option<String>,
	pub approved: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Qualification {
	#[serde(deserialize_with = "required")]
	pub report: Option<String>,
	#[serde(deserialize_with = "required")]
	pub reviewer: Option<String>,
}

/// Mirrors `schemas/model-manifest.schema.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelManifest {
	pub schema_version: u32,
	pub backend_id: String,
	pub status: Status,
	pub source: Source,
	pub model: Model,
	pub runtime: Runtime,
	pub audio: AudioSpec,
	pub distribution: Distribution,
	pub qualification: Qualification,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManifestError {
	#[error("manifest JSON is invalid: {0}")]
	Parse(String),
	#[error("unsupported schema_version {0}")]
	SchemaVersion(u32),
	#[error("{0}")]
	Invalid(String),
}

fn is_sha256(value: &str) -> bool {
	value.len() == 64 && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn non_empty(value: &Option<String>) -> bool {
	value.as_deref().is_some_and(|v| !v.is_empty())
}

impl ModelManifest {
	pub fn from_json(text: &str) -> Result<Self, ManifestError> {
		let manifest: Self =
			serde_json::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?;
		manifest.validate()?;
		Ok(manifest)
	}

	/// Check the schema's value constraints, including the extra
	/// requirements that apply once `status` is `qualified`.
	pub fn validate(&self) -> Result<(), ManifestError> {
		let bad = |msg: &str| Err(ManifestError::Invalid(msg.to_owned()));
		if self.schema_version != 1 {
			return Err(ManifestError::SchemaVersion(self.schema_version));
		}
		if self.backend_id.is_empty() {
			return bad("backend_id must not be empty");
		}
		if self.runtime.threads < 1 {
			return bad("runtime.threads must be at least 1");
		}
		if let Some(hash) = &self.model.sha256 {
			if !is_sha256(hash) {
				return bad("model.sha256 must be 64 lowercase hex characters");
			}
		}
		if self.audio.sample_rate_hz.is_some_and(|r| r < 8000) {
			return bad("audio.sample_rate_hz must be at least 8000");
		}
		if self.audio.hop_samples == Some(0) || self.audio.analysis_window_samples == Some(0) {
			return bad("audio hop and window must be at least 1");
		}
		if self.audio.measured_total_delay_ms.is_some_and(|d| !(d >= 0.0)) {
			return bad("audio.measured_total_delay_ms must be non-negative");
		}
		if self.status == Status::Qualified {
			self.validate_qualified()?;
		}
		Ok(())
	}

	fn validate_qualified(&self) -> Result<(), ManifestError> {
		let missing = |field: &str| {
			Err(ManifestError::Invalid(format!(
				"qualified manifest requires {field}"
			)))
		};
		if !non_empty(&self.source.revision) {
			return missing("source.revision");
		}
		if self.model.sha256.is_none() {
			return missing("model.sha256");
		}
		if self.audio.sample_rate_hz.is_none()
			|| self.audio.hop_samples.is_none()
			|| self.audio.analysis_window_samples.is_none()
		{
			return missing("audio rate, hop and window");
		}
		if self.audio.measured_total_delay_ms.is_none() {
			return missing("audio.measured_total_delay_ms");
		}
		if !self.distribution.approved {
			return missing("distribution.approved = true");
		}
		if !non_empty(&self.distribution.code_license) || !non_empty(&self.distribution.model_license)
		{
			return missing("code and model licenses");
		}
		if !non_empty(&self.qualification.report) || !non_empty(&self.qualification.reviewer) {
			return missing("qualification report and reviewer");
		}
		if self.runtime.name.is_empty() || !non_empty(&self.runtime.version) {
			return missing("runtime name and version");
		}
		Ok(())
	}
}
