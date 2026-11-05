//! cpal-backed devices. Platform handles never leave this module.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, ErrorKind, SampleFormat, SizedSample, StreamConfig, SupportedBufferSize};

use super::{
	ActualConfig, AudioBackend, CaptureStream, ConfigRange, DeviceDescriptor, DeviceHandle,
	Direction, Enumeration, PlaybackStream, SampleFormatKind, StableDeviceKey, StreamHandle,
	StreamHealth, StreamRequest, StreamState, ring_samples_for,
};
use crate::capture::{CaptureCallback, CaptureSetup, capture_pair};
use crate::convert::Sample;
use crate::error::{AudioError, Result};
use crate::playback::{PlaybackCallback, playback_pair};

/// Callback size assumed when the backend cannot declare a maximum. Larger
/// callbacks are still safe: overflow is counted, never written past.
const ASSUMED_MAX_CALLBACK: u32 = 4096;

fn backend_err(error: cpal::Error) -> AudioError {
	AudioError::Backend(error.to_string())
}

fn kind_of(format: SampleFormat) -> Option<SampleFormatKind> {
	match format {
		SampleFormat::F32 => Some(SampleFormatKind::F32),
		SampleFormat::I16 => Some(SampleFormatKind::I16),
		SampleFormat::U16 => Some(SampleFormatKind::U16),
		SampleFormat::I32 => Some(SampleFormatKind::I32),
		_ => None,
	}
}

fn rank(kind: SampleFormatKind) -> u8 {
	match kind {
		SampleFormatKind::F32 => 0,
		SampleFormatKind::I16 => 1,
		SampleFormatKind::I32 => 2,
		SampleFormatKind::U16 => 3,
	}
}

/// Maps a cpal stream error onto the stream's health cell (atomics only).
pub fn report_cpal_error(health: &StreamHealth, error: &cpal::Error) {
	match error.kind() {
		ErrorKind::DeviceNotAvailable => health.report(StreamState::DeviceLost),
		ErrorKind::StreamInvalidated | ErrorKind::DeviceChanged => {
			health.report(StreamState::Invalidated);
		}
		ErrorKind::Xrun => health.report_xrun(),
		// Audio still flows without the promotion; the owner reads the flag and logs it.
		ErrorKind::RealtimeDenied => health.report_realtime_denied(),
		_ => health.report(StreamState::BackendError),
	}
}

fn health_callback(health: StreamHealth) -> impl FnMut(cpal::Error) + Send + 'static {
	move |error| report_cpal_error(&health, &error)
}

struct CpalStream(cpal::Stream);

impl StreamHandle for CpalStream {
	fn play(&self) -> Result<()> {
		self.0.play().map_err(backend_err)
	}

	fn pause(&self) -> Result<()> {
		self.0.pause().map_err(backend_err)
	}
}

/// The platform's default host through cpal.
pub struct CpalBackend {
	host: cpal::Host,
	generation: u64,
	entries: Vec<(Direction, cpal::Device)>,
}

impl Default for CpalBackend {
	fn default() -> Self {
		Self::new()
	}
}

impl CpalBackend {
	pub fn new() -> Self {
		Self {
			host: cpal::default_host(),
			generation: 0,
			entries: Vec::new(),
		}
	}

	fn device(&self, handle: DeviceHandle, direction: Direction) -> Result<&cpal::Device> {
		if handle.enumeration != self.generation {
			return Err(AudioError::DeviceNotFound);
		}
		self.entries
			.get(handle.index as usize)
			.filter(|(d, _)| *d == direction)
			.map(|(_, device)| device)
			.ok_or(AudioError::DeviceNotFound)
	}

	/// Pick a supported configuration near the request, else the default.
	fn choose(
		device: &cpal::Device,
		direction: Direction,
		request: &StreamRequest,
	) -> Result<(StreamConfig, SampleFormatKind, u32)> {
		let ranges: Vec<_> = match direction {
			Direction::Input => device
				.supported_input_configs()
				.map_err(backend_err)?
				.collect(),
			Direction::Output => device
				.supported_output_configs()
				.map_err(backend_err)?
				.collect(),
		};
		let needed = match request.select {
			crate::convert::ChannelSelect::Channel(i) => i + 1,
			crate::convert::ChannelSelect::Average => 1,
		};
		let wanted_channels = request.channels.unwrap_or(match direction {
			Direction::Input => needed,
			Direction::Output => 1,
		});
		let best = ranges
			.iter()
			.filter(|r| r.contains_rate(request.sample_rate) && r.channels() >= wanted_channels)
			.filter_map(|r| kind_of(r.sample_format()).map(|k| (r, k)))
			.min_by_key(|(r, k)| (rank(*k), r.channels()));
		let supported = match best {
			Some((range, _)) => range
				.try_with_sample_rate(request.sample_rate)
				.ok_or_else(|| AudioError::UnsupportedFormat("rate".into()))?,
			None => match direction {
				Direction::Input => device.default_input_config(),
				Direction::Output => device.default_output_config(),
			}
			.map_err(backend_err)?,
		};
		let kind = kind_of(supported.sample_format())
			.ok_or_else(|| AudioError::UnsupportedFormat("sample format".into()))?;
		let max_callback = match (request.buffer_frames, supported.buffer_size()) {
			(Some(n), _) => n,
			(None, SupportedBufferSize::Range { max, .. }) => (*max).min(ASSUMED_MAX_CALLBACK),
			(None, SupportedBufferSize::Unknown) => ASSUMED_MAX_CALLBACK,
		};
		let config = StreamConfig {
			channels: supported.channels(),
			sample_rate: supported.sample_rate(),
			buffer_size: request
				.buffer_frames
				.map_or(BufferSize::Default, BufferSize::Fixed),
		};
		Ok((config, kind, max_callback))
	}
}

fn build_input<T: SizedSample + Sample>(
	device: &cpal::Device,
	config: StreamConfig,
	mut callback: CaptureCallback,
	health: StreamHealth,
) -> Result<cpal::Stream> {
	device
		.build_input_stream::<T, _, _>(
			config,
			move |data, _| callback.process(data),
			health_callback(health),
			None,
		)
		.map_err(backend_err)
}

fn build_output<T: SizedSample + Sample>(
	device: &cpal::Device,
	config: StreamConfig,
	mut callback: PlaybackCallback,
	health: StreamHealth,
) -> Result<cpal::Stream> {
	device
		.build_output_stream::<T, _, _>(
			config,
			move |data, _| callback.process(data),
			health_callback(health),
			None,
		)
		.map_err(backend_err)
}

impl AudioBackend for CpalBackend {
	fn enumerate(&mut self) -> Result<Enumeration> {
		self.generation += 1;
		self.entries.clear();
		let host_name = self.host.id().name().to_ascii_lowercase();
		let default_in = self.host.default_input_device().and_then(|d| d.id().ok());
		let default_out = self.host.default_output_device().and_then(|d| d.id().ok());
		let mut devices = Vec::new();
		for device in self.host.devices().map_err(backend_err)? {
			let Ok(description) = device.description() else {
				continue;
			};
			let id = device.id().ok();
			for direction in [Direction::Input, Direction::Output] {
				let ranges: Vec<_> = match direction {
					Direction::Input => device.supported_input_configs().map(Iterator::collect),
					Direction::Output => device.supported_output_configs().map(Iterator::collect),
				}
				.unwrap_or_default();
				if ranges.is_empty() {
					continue;
				}
				let default = match direction {
					Direction::Input => &default_in,
					Direction::Output => &default_out,
				};
				let handle = DeviceHandle {
					enumeration: self.generation,
					index: self.entries.len() as u32,
				};
				devices.push(DeviceDescriptor {
					handle,
					key: StableDeviceKey {
						host: host_name.clone(),
						direction,
						backend_id: id.as_ref().map(ToString::to_string),
						name: description.name().to_owned(),
						manufacturer: description.manufacturer().map(str::to_owned),
						interface: Some(format!("{:?}", description.interface_type())),
					},
					is_default: id.is_some() && *default == id,
					configs: ranges
						.iter()
						.map(|r| ConfigRange {
							channels: r.channels(),
							min_rate: r.min_sample_rate(),
							max_rate: r.max_sample_rate(),
							format: kind_of(r.sample_format()),
							max_buffer_frames: match r.buffer_size() {
								SupportedBufferSize::Range { max, .. } => Some(*max),
								SupportedBufferSize::Unknown => None,
							},
						})
						.collect(),
				});
				self.entries.push((direction, device.clone()));
			}
		}
		Ok(Enumeration {
			generation: self.generation,
			devices,
		})
	}

	fn open_capture(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> Result<CaptureStream> {
		let device = self.device(device, Direction::Input)?;
		let (config, kind, max_callback) = Self::choose(device, Direction::Input, request)?;
		if !request.select.is_valid_for(config.channels) {
			return Err(AudioError::UnsupportedFormat(
				"channel selection out of range".into(),
			));
		}
		let (callback, reader, shared) = capture_pair(CaptureSetup {
			channels: config.channels,
			select: request.select,
			ring_samples: ring_samples_for(config.sample_rate, max_callback),
		});
		let health = StreamHealth::default();
		let h = health.clone();
		let stream = match kind {
			SampleFormatKind::F32 => build_input::<f32>(device, config, callback, h),
			SampleFormatKind::I16 => build_input::<i16>(device, config, callback, h),
			SampleFormatKind::U16 => build_input::<u16>(device, config, callback, h),
			SampleFormatKind::I32 => build_input::<i32>(device, config, callback, h),
		}?;
		Ok(CaptureStream {
			config: ActualConfig {
				sample_rate: config.sample_rate,
				channels: config.channels,
				format: kind,
				buffer_frames: request.buffer_frames,
				max_callback_frames: max_callback,
			},
			reader,
			shared,
			health,
			handle: Box::new(CpalStream(stream)),
		})
	}

	fn open_playback(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> Result<PlaybackStream> {
		let device = self.device(device, Direction::Output)?;
		let (config, kind, max_callback) = Self::choose(device, Direction::Output, request)?;
		let (callback, writer, shared) = playback_pair(
			config.channels,
			ring_samples_for(config.sample_rate, max_callback),
		);
		let health = StreamHealth::default();
		let h = health.clone();
		let stream = match kind {
			SampleFormatKind::F32 => build_output::<f32>(device, config, callback, h),
			SampleFormatKind::I16 => build_output::<i16>(device, config, callback, h),
			SampleFormatKind::U16 => build_output::<u16>(device, config, callback, h),
			SampleFormatKind::I32 => build_output::<i32>(device, config, callback, h),
		}?;
		Ok(PlaybackStream {
			config: ActualConfig {
				sample_rate: config.sample_rate,
				channels: config.channels,
				format: kind,
				buffer_frames: request.buffer_frames,
				max_callback_frames: max_callback,
			},
			writer,
			shared,
			health,
			handle: Box::new(CpalStream(stream)),
		})
	}
}
