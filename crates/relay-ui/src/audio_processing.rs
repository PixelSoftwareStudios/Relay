//! Microphone processing switches: noise suppression, its strength and automatic gain. They are
//! the preferences `audio.noise_suppression`, `audio.noise_suppression_level` and
//! `audio.auto_gain`, sent with `SetSettings`; the engine applies them to the live microphone
//! path and answers per key.

use serde::Serialize;

use crate::controller::{ControlError, Controller};
use crate::engine::SettingOutcome;

pub const KEY_NOISE_SUPPRESSION: &str = "audio.noise_suppression";
pub const KEY_NOISE_LEVEL: &str = "audio.noise_suppression_level";
pub const KEY_AUTO_GAIN: &str = "audio.auto_gain";

/// How hard noise suppression works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NoiseLevel {
	Low,
	Moderate,
	High,
}

impl NoiseLevel {
	pub const ALL: [Self; 3] = [Self::Low, Self::Moderate, Self::High];

	/// The preference value.
	pub fn key(self) -> &'static str {
		match self {
			Self::Low => "low",
			Self::Moderate => "moderate",
			Self::High => "high",
		}
	}

	/// An unknown value reads as the default strength.
	pub fn from_key(key: &str) -> Self {
		Self::ALL
			.into_iter()
			.find(|level| level.key() == key)
			.unwrap_or(Self::Moderate)
	}
}

/// One change of the microphone processing; `None` leaves a setting as it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessingChange {
	pub noise_suppression: Option<bool>,
	pub level: Option<NoiseLevel>,
	pub auto_gain: Option<bool>,
}

impl ProcessingChange {
	fn entries(self) -> Vec<(String, String)> {
		let mut entries = Vec::new();
		if let Some(on) = self.noise_suppression {
			entries.push((KEY_NOISE_SUPPRESSION.to_owned(), on.to_string()));
		}
		if let Some(level) = self.level {
			entries.push((KEY_NOISE_LEVEL.to_owned(), level.key().to_owned()));
		}
		if let Some(on) = self.auto_gain {
			entries.push((KEY_AUTO_GAIN.to_owned(), on.to_string()));
		}
		entries
	}
}

impl Controller {
	/// Changes the microphone processing. A key the engine refuses is an error carrying its reason.
	pub async fn set_audio_processing(&self, change: ProcessingChange) -> Result<(), ControlError> {
		let session = self.slot.get().ok_or(ControlError::NotConnected)?;
		let results = session.ops.set_settings(change.entries()).await?;
		match results
			.into_iter()
			.find(|r| r.outcome == SettingOutcome::Rejected)
		{
			Some(refused) => Err(ControlError::Engine(refused.reason)),
			None => Ok(()),
		}
	}
}
