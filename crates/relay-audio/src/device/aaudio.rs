//! Android devices through AAudio with voice-communication routing.
//!
//! cpal's AAudio host cannot choose the input preset or the usage, so on
//! Android the engine opens its streams here: capture with
//! `AAUDIO_INPUT_PRESET_VOICE_COMMUNICATION` (platform echo cancellation,
//! noise suppression and gain control apply where the device provides them)
//! and playback with `AAUDIO_USAGE_VOICE_COMMUNICATION`. Which physical
//! microphone or speaker is used is the system's routing decision
//! (`AudioManager`), so exactly one input and one output are listed.

use ndk::audio::{
	AudioCallbackResult, AudioDirection, AudioError as NdkError, AudioFormat, AudioInputPreset,
	AudioPerformanceMode, AudioSharingMode, AudioStream, AudioStreamBuilder, AudioUsage,
};

use super::{
	ActualConfig, AudioBackend, CaptureStream, ConfigRange, DeviceDescriptor, DeviceHandle,
	Direction, Enumeration, PlaybackStream, SampleFormatKind, StableDeviceKey, StreamHandle,
	StreamHealth, StreamRequest, StreamState, ring_samples_for,
};
use crate::capture::{CaptureSetup, capture_pair};
use crate::error::{AudioError, Result};
use crate::playback::playback_pair;

const HOST: &str = "aaudio";
/// Callback size assumed when the request does not fix one.
const ASSUMED_MAX_CALLBACK: u32 = 4096;
const INPUT_INDEX: u32 = 0;
const OUTPUT_INDEX: u32 = 1;

fn backend_err(error: NdkError) -> AudioError {
	AudioError::Backend(error.to_string())
}

fn report(health: &StreamHealth, error: NdkError) {
	health.report(match error {
		NdkError::Disconnected => StreamState::DeviceLost,
		_ => StreamState::BackendError,
	});
}

/// Bursts of output buffer to run with (the native burst is the callback size).
const OUTPUT_BURSTS: i32 = 2;

/// The callback size to plan for: what the stream reports, else its burst.
fn callback_hint(stream: &AudioStream) -> Option<u32> {
	let frames = stream
		.frames_per_data_callback()
		.filter(|f| *f > 0)
		.unwrap_or_else(|| stream.frames_per_burst());
	u32::try_from(frames).ok().filter(|f| *f > 0)
}

struct AAudioHandle(AudioStream);

impl StreamHandle for AAudioHandle {
	fn play(&self) -> Result<()> {
		self.0.request_start().map_err(backend_err)
	}

	fn pause(&self) -> Result<()> {
		self.0.request_pause().map_err(backend_err)
	}
}

/// The Android default microphone and output, opened for a voice call.
#[derive(Debug, Default)]
pub struct AAudioBackend {
	generation: u64,
}

impl AAudioBackend {
	pub fn new() -> Self {
		Self::default()
	}

	fn check(&self, handle: DeviceHandle, index: u32) -> Result<()> {
		if handle.enumeration == self.generation && handle.index == index {
			Ok(())
		} else {
			Err(AudioError::DeviceNotFound)
		}
	}

	fn descriptor(&self, direction: Direction, index: u32, name: &str) -> DeviceDescriptor {
		DeviceDescriptor {
			handle: DeviceHandle {
				enumeration: self.generation,
				index,
			},
			key: StableDeviceKey {
				host: HOST.to_owned(),
				direction,
				backend_id: Some(format!("{direction:?}-default").to_ascii_lowercase()),
				name: name.to_owned(),
				manufacturer: None,
				interface: None,
			},
			is_default: true,
			configs: vec![ConfigRange {
				channels: 1,
				min_rate: 8_000,
				max_rate: 48_000,
				format: Some(SampleFormatKind::I16),
				max_buffer_frames: None,
			}],
		}
	}
}

impl AudioBackend for AAudioBackend {
	fn enumerate(&mut self) -> Result<Enumeration> {
		self.generation += 1;
		Ok(Enumeration {
			generation: self.generation,
			devices: vec![
				self.descriptor(
					Direction::Input,
					INPUT_INDEX,
					"Phone microphone (voice call)",
				),
				self.descriptor(
					Direction::Output,
					OUTPUT_INDEX,
					"Phone audio output (voice call)",
				),
			],
		})
	}

	fn open_capture(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> Result<CaptureStream> {
		self.check(device, INPUT_INDEX)?;
		if !request.select.is_valid_for(1) {
			return Err(AudioError::UnsupportedFormat(
				"channel selection out of range".into(),
			));
		}
		let max_callback = request.buffer_frames.unwrap_or(ASSUMED_MAX_CALLBACK);
		let (mut callback, reader, shared) = capture_pair(CaptureSetup {
			channels: 1,
			select: request.select,
			ring_samples: ring_samples_for(request.sample_rate, max_callback),
		});
		let health = StreamHealth::default();
		let on_error = health.clone();
		let builder = AudioStreamBuilder::new()
			.map_err(backend_err)?
			.direction(AudioDirection::Input)
			.sample_rate(request.sample_rate as i32)
			.channel_count(1)
			.format(AudioFormat::PCM_I16)
			.input_preset(AudioInputPreset::VoiceCommunication)
			.performance_mode(AudioPerformanceMode::LowLatency)
			.sharing_mode(AudioSharingMode::Shared)
			.data_callback(Box::new(move |_, data, frames| {
				// SAFETY: this PCM_I16 mono stream supplies an aligned buffer of `frames`
				// readable samples for this callback. AAudio supplies a positive frame count;
				// the borrowed slice is consumed synchronously and never escapes the callback.
				let samples =
					unsafe { std::slice::from_raw_parts(data.cast::<i16>(), frames as usize) };
				callback.process(samples);
				AudioCallbackResult::Continue
			}))
			.error_callback(Box::new(move |_, error| report(&on_error, error)));
		let stream = builder.open_stream().map_err(backend_err)?;
		tracing::info!(
			rate = stream.sample_rate(),
			preset = ?stream.input_preset(),
			mode = ?stream.performance_mode(),
			burst = stream.frames_per_burst(),
			"AAudio capture opened"
		);
		Ok(CaptureStream {
			config: ActualConfig {
				sample_rate: stream.sample_rate() as u32,
				channels: 1,
				format: SampleFormatKind::I16,
				buffer_frames: Some(stream.buffer_size_in_frames() as u32),
				callback_frames: callback_hint(&stream),
				max_callback_frames: max_callback,
			},
			reader,
			shared,
			health,
			handle: Box::new(AAudioHandle(stream)),
		})
	}

	fn open_playback(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> Result<PlaybackStream> {
		self.check(device, OUTPUT_INDEX)?;
		let channels = request.channels.unwrap_or(1).max(1);
		let max_callback = request.buffer_frames.unwrap_or(ASSUMED_MAX_CALLBACK);
		let (mut callback, writer, shared) = playback_pair(
			channels,
			ring_samples_for(request.sample_rate, max_callback),
		);
		let health = StreamHealth::default();
		let on_error = health.clone();
		let width = usize::from(channels);
		let builder = AudioStreamBuilder::new()
			.map_err(backend_err)?
			.direction(AudioDirection::Output)
			.sample_rate(request.sample_rate as i32)
			.channel_count(i32::from(channels))
			.format(AudioFormat::PCM_I16)
			.usage(AudioUsage::VoiceCommunication)
			.performance_mode(AudioPerformanceMode::LowLatency)
			.sharing_mode(AudioSharingMode::Shared)
			.data_callback(Box::new(move |_, data, frames| {
				// SAFETY: this PCM_I16 stream was configured with `width` channels. AAudio
				// supplies exclusive access to an aligned output buffer of `frames * width`
				// samples for this invocation. Both counts describe the allocated device
				// buffer, and the mutable slice does not escape the callback.
				let samples = unsafe {
					std::slice::from_raw_parts_mut(data.cast::<i16>(), frames as usize * width)
				};
				callback.process(samples);
				AudioCallbackResult::Continue
			}))
			.error_callback(Box::new(move |_, error| report(&on_error, error)));
		let opening = std::time::Instant::now();
		let stream = builder.open_stream().map_err(backend_err)?;
		let open_ms = u64::try_from(opening.elapsed().as_millis()).unwrap_or(u64::MAX);
		// Low latency means a buffer of a few bursts: two bursts absorb the
		// render worker's 10 ms schedule without the default's deep queue.
		let burst = stream.frames_per_burst();
		let sized = stream.set_buffer_size_in_frames(burst * OUTPUT_BURSTS);
		tracing::info!(
			rate = stream.sample_rate(),
			usage = ?stream.usage(),
			mode = ?stream.performance_mode(),
			burst,
			buffer = ?sized,
			open_ms,
			"AAudio playback opened"
		);
		Ok(PlaybackStream {
			config: ActualConfig {
				sample_rate: stream.sample_rate() as u32,
				channels,
				format: SampleFormatKind::I16,
				buffer_frames: Some(stream.buffer_size_in_frames() as u32),
				callback_frames: callback_hint(&stream),
				max_callback_frames: max_callback,
			},
			writer,
			shared,
			health,
			handle: Box::new(AAudioHandle(stream)),
		})
	}
}
