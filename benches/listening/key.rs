//! The secret mapping from neutral clip names to what they really are.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::Result;

pub const KEY_DIR: &str = "key";
pub const KEY_FILE: &str = "key.json";
pub const ANSWERS_FILE: &str = "answers.csv";
pub const CLIP_DIR: &str = "clips";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClipKey {
	/// Neutral clip name without extension, e.g. `trial03_B`.
	pub clip: String,
	pub trial: usize,
	pub utterance: String,
	/// Input condition, e.g. `snr05` or `clean`.
	pub condition: String,
	pub backend: String,
	/// Gain applied so the clip matches the unprocessed clip's loudness.
	pub gain_db: f64,
	/// Delay removed so all clips of a trial line up, in 48 kHz samples.
	pub delay_removed_samples: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Key {
	pub schema_version: u32,
	pub seed: u64,
	pub backends: Vec<String>,
	pub clips: Vec<ClipKey>,
}

impl Key {
	pub fn path(session: &Path) -> PathBuf {
		session.join(KEY_DIR).join(KEY_FILE)
	}

	pub fn save(&self, session: &Path) -> Result<()> {
		std::fs::create_dir_all(session.join(KEY_DIR))?;
		std::fs::write(Self::path(session), serde_json::to_string_pretty(self)?)?;
		Ok(())
	}

	pub fn load(session: &Path) -> Result<Self> {
		Ok(serde_json::from_str(&std::fs::read_to_string(
			Self::path(session),
		)?)?)
	}
}
