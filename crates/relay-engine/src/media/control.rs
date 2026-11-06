//! Service port implementations for the media system.

use relay_audio::FRAME_SAMPLES;
use relay_audio::device::{DeviceDescriptor, DeviceSelection, Direction};
use relay_audio::dsp::{BackendKind, DspLimits, create_suppressor};
use relay_core::{AudioGeneration, CallId, ReceiveMix, SessionId};

use super::capture_worker::CaptureCmd;
use super::devices::Selection;
use super::system::{MediaSystem, PrepareError};
use crate::service::ToneCommand;
use crate::service::ports::{
	DeviceControl, DeviceDirection, DeviceInfo, MediaControl, MediaTelemetry, PortError,
};

impl MediaControl for MediaSystem {
	fn open_capture(&self, generation: AudioGeneration) -> Result<(), PortError> {
		MediaSystem::open_capture(self, generation);
		Ok(())
	}

	fn close_capture(&self, generation: AudioGeneration) {
		MediaSystem::close_capture(self, generation);
	}

	fn prepare_media(&self, call: CallId, session: SessionId) -> Result<(), PortError> {
		self.prepare(call, session).map_err(|error| match error {
			PrepareError::Refused(refusal) => PortError::Unavailable(refusal.to_string()),
			PrepareError::UnknownCall => PortError::NotFound(error.to_string()),
			PrepareError::Duplicate => PortError::Invalid(error.to_string()),
			PrepareError::Stopped => PortError::Unavailable(error.to_string()),
		})
	}

	fn stop_media(&self, call: CallId, session: SessionId) {
		self.stop(call, session);
	}

	fn set_receive_mix(&self, mix: ReceiveMix) {
		MediaSystem::set_receive_mix(self, mix);
	}

	/// Only `off` is selectable today: every other backend needs a qualified,
	/// approved model, which `create_suppressor` refuses without one.
	fn set_noise_suppression(&self, backend: &str) -> Result<(), PortError> {
		let kind = match backend {
			"" | "off" => BackendKind::Off,
			"rnnoise" => BackendKind::Rnnoise,
			"dtln" => BackendKind::Dtln,
			other => return Err(PortError::Invalid(format!("unknown backend `{other}`"))),
		};
		let mut suppressor = create_suppressor(kind, None)
			.map_err(|error| PortError::Unavailable(error.to_string()))?;
		let limits = DspLimits {
			max_block_samples: FRAME_SAMPLES,
			max_scratch_bytes: suppressor.descriptor().max_scratch_bytes,
		};
		suppressor
			.prepare(&limits)
			.map_err(|error| PortError::Unavailable(error.to_string()))?;
		self.inner
			.capture
			.push(CaptureCmd::SetSuppressor(suppressor))
			.map_err(|_| PortError::Unavailable("capture queue is full".to_owned()))
	}

	fn set_microphone_gain(&self, gain: f32) -> Result<(), PortError> {
		if !gain.is_finite() || !(0.0..=4.0).contains(&gain) {
			return Err(PortError::Invalid(
				"gain must be within 0.0..=4.0".to_owned(),
			));
		}
		self.inner
			.capture
			.push(CaptureCmd::SetGain(gain))
			.map_err(|_| PortError::Unavailable("capture queue is full".to_owned()))
	}

	fn start_mic_test(&self, local_playback: bool) -> Result<(), PortError> {
		MediaSystem::start_mic_test(self, local_playback)
	}

	fn stop_mic_test(&self) -> Result<(), PortError> {
		MediaSystem::stop_mic_test(self);
		Ok(())
	}

	fn telemetry(&self) -> MediaTelemetry {
		MediaSystem::telemetry(self)
	}

	fn call_stats(&self, call: CallId) -> Option<crate::service::call_stats::CallRaw> {
		MediaSystem::call_raw(self, call)
	}

	fn tone(&self, command: ToneCommand) {
		MediaSystem::tone(self, command);
	}
}

/// The stable text id the API uses for a device.
pub fn device_id(device: &DeviceDescriptor) -> String {
	let key = &device.key;
	let direction = match key.direction {
		Direction::Input => "in",
		Direction::Output => "out",
	};
	format!(
		"{direction}:{}:{}",
		key.host,
		key.backend_id.as_deref().unwrap_or(&key.name)
	)
}

impl DeviceControl for MediaSystem {
	fn devices(&self) -> Vec<DeviceInfo> {
		self.device_listing()
			.iter()
			.map(|device| {
				let mut rates: Vec<u32> = device
					.configs
					.iter()
					.flat_map(|c| [c.min_rate, c.max_rate])
					.collect();
				rates.sort_unstable();
				rates.dedup();
				DeviceInfo {
					id: device_id(device),
					name: device.key.name.clone(),
					direction: match device.key.direction {
						Direction::Input => DeviceDirection::Input,
						Direction::Output => DeviceDirection::Output,
					},
					available: true,
					system_default: device.is_default,
					rates,
				}
			})
			.collect()
	}

	/// An empty id follows the system default for that direction. A named
	/// device that is not listed is an error, never replaced by another.
	fn select(&self, input: &str, output: &str) -> Result<(), PortError> {
		let listing = self.device_listing();
		let find = |id: &str, direction: Direction| {
			listing
				.iter()
				.find(|d| d.key.direction == direction && device_id(d) == id)
				.map(|d| DeviceSelection::Pinned(d.key.clone()))
				.ok_or_else(|| PortError::NotFound(format!("device {id}")))
		};
		let input = if input.is_empty() {
			DeviceSelection::FollowDefault
		} else {
			find(input, Direction::Input)?
		};
		let output = if output.is_empty() {
			DeviceSelection::FollowDefault
		} else {
			find(output, Direction::Output)?
		};
		self.select_devices(Selection { input, output });
		Ok(())
	}
}
