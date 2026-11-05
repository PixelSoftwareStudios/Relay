//! Deterministic, hardware-free backend for tests and loopback runs.

use super::{
	ActualConfig, AudioBackend, CaptureStream, ConfigRange, DeviceDescriptor, DeviceHandle,
	Direction, Enumeration, PlaybackStream, SampleFormatKind, StableDeviceKey, StreamHandle,
	StreamHealth, StreamRequest, StreamState, ring_samples_for,
};
use crate::capture::{CaptureCallback, CaptureSetup, capture_pair};
use crate::convert::Sample;
use crate::error::{AudioError, Result};
use crate::playback::{PlaybackCallback, playback_pair};

/// A scripted device.
#[derive(Debug, Clone)]
pub struct FakeDeviceSpec {
	pub name: String,
	pub backend_id: Option<String>,
	pub direction: Direction,
	pub is_default: bool,
	pub channels: u16,
	pub sample_rate: u32,
	pub format: SampleFormatKind,
	pub max_callback_frames: u32,
}

impl FakeDeviceSpec {
	pub fn new(name: &str, direction: Direction) -> Self {
		Self {
			name: name.to_owned(),
			backend_id: Some(format!("fake:{name}")),
			direction,
			is_default: false,
			channels: 1,
			sample_rate: 48_000,
			format: SampleFormatKind::F32,
			max_callback_frames: 512,
		}
	}
}

/// Deterministic test signal as a function of the sample index.
#[derive(Debug, Clone, Copy)]
pub enum Signal {
	Silence,
	Sine {
		freq_hz: f32,
		amplitude: f32,
	},
	/// Slowly increasing values: `((index % 1000) + 1) / 2000`. Useful to
	/// verify exact sample accounting.
	Ramp,
	Impulse {
		at: u64,
		amplitude: f32,
	},
}

impl Signal {
	pub fn value(self, index: u64, rate: u32) -> f32 {
		match self {
			Self::Silence => 0.0,
			Self::Sine { freq_hz, amplitude } => {
				let phase = (index as f64) * f64::from(freq_hz) / f64::from(rate);
				amplitude * (phase.fract() * std::f64::consts::TAU).sin() as f32
			}
			Self::Ramp => ((index % 1000) + 1) as f32 / 2000.0,
			Self::Impulse { at, amplitude } => {
				if index == at {
					amplitude
				} else {
					0.0
				}
			}
		}
	}
}

struct NoopHandle;

impl StreamHandle for NoopHandle {
	fn play(&self) -> Result<()> {
		Ok(())
	}

	fn pause(&self) -> Result<()> {
		Ok(())
	}
}

/// Test-side controller of a fake capture device.
pub struct FakeCaptureDriver {
	callback: CaptureCallback,
	config: ActualConfig,
	health: StreamHealth,
	signal: Signal,
	next_index: u64,
	f32_buf: Vec<f32>,
	i16_buf: Vec<i16>,
	unplugged: bool,
}

impl FakeCaptureDriver {
	pub fn set_signal(&mut self, signal: Signal) {
		self.signal = signal;
	}

	/// Index of the next device frame to be generated.
	pub fn next_index(&self) -> u64 {
		self.next_index
	}

	/// Simulate one hardware callback of `frames` frames (at most the
	/// configured maximum). Odd channels carry the inverted signal so
	/// channel selection is observable. Allocation-free.
	pub fn run(&mut self, frames: usize) {
		if self.unplugged {
			return;
		}
		let channels = usize::from(self.config.channels);
		let frames = frames.min(self.config.max_callback_frames as usize);
		for frame in 0..frames {
			let value = self.signal.value(self.next_index, self.config.sample_rate);
			self.next_index += 1;
			for ch in 0..channels {
				let v = if ch % 2 == 0 { value } else { -value };
				match self.config.format {
					SampleFormatKind::I16 => self.i16_buf[frame * channels + ch] = i16::from_f32(v),
					_ => self.f32_buf[frame * channels + ch] = v,
				}
			}
		}
		match self.config.format {
			SampleFormatKind::I16 => self.callback.process(&self.i16_buf[..frames * channels]),
			_ => self.callback.process(&self.f32_buf[..frames * channels]),
		}
	}

	/// Remove the device: callbacks stop and the health cell reports it.
	pub fn unplug(&mut self) {
		self.unplugged = true;
		self.health.report(StreamState::DeviceLost);
	}
}

/// Test-side controller of a fake render device.
pub struct FakePlaybackDriver {
	callback: PlaybackCallback,
	config: ActualConfig,
	health: StreamHealth,
	f32_buf: Vec<f32>,
	i16_buf: Vec<i16>,
	recorded: Vec<f32>,
	record_limit: usize,
	unplugged: bool,
}

impl FakePlaybackDriver {
	/// Keep up to `limit` rendered frames (first channel). Allocates once.
	pub fn enable_recording(&mut self, limit: usize) {
		self.recorded = Vec::with_capacity(limit);
		self.record_limit = limit;
	}

	pub fn recorded(&self) -> &[f32] {
		&self.recorded
	}

	/// Simulate one hardware callback requesting `frames` frames.
	pub fn render(&mut self, frames: usize) {
		if self.unplugged {
			return;
		}
		let channels = usize::from(self.config.channels);
		let frames = frames.min(self.config.max_callback_frames as usize);
		match self.config.format {
			SampleFormatKind::I16 => {
				let out = &mut self.i16_buf[..frames * channels];
				self.callback.process(out);
				for frame in out.chunks_exact(channels) {
					if self.recorded.len() < self.record_limit {
						self.recorded.push(frame[0].to_f32());
					}
				}
			}
			_ => {
				let out = &mut self.f32_buf[..frames * channels];
				self.callback.process(out);
				for frame in out.chunks_exact(channels) {
					if self.recorded.len() < self.record_limit {
						self.recorded.push(frame[0]);
					}
				}
			}
		}
	}

	pub fn unplug(&mut self) {
		self.unplugged = true;
		self.health.report(StreamState::DeviceLost);
	}
}

/// Hardware-free [`AudioBackend`].
pub struct FakeBackend {
	specs: Vec<FakeDeviceSpec>,
	generation: u64,
	capture_driver: Option<FakeCaptureDriver>,
	playback_driver: Option<FakePlaybackDriver>,
}

impl FakeBackend {
	pub fn new(specs: Vec<FakeDeviceSpec>) -> Self {
		Self {
			specs,
			generation: 0,
			capture_driver: None,
			playback_driver: None,
		}
	}

	/// Simulate hot-plug: remove a device by name before the next scan.
	pub fn remove_device(&mut self, name: &str) {
		self.specs.retain(|s| s.name != name);
	}

	pub fn add_device(&mut self, spec: FakeDeviceSpec) {
		self.specs.push(spec);
	}

	/// Driver for the most recently opened capture stream.
	pub fn take_capture_driver(&mut self) -> Option<FakeCaptureDriver> {
		self.capture_driver.take()
	}

	pub fn take_playback_driver(&mut self) -> Option<FakePlaybackDriver> {
		self.playback_driver.take()
	}

	fn spec(&self, device: DeviceHandle, direction: Direction) -> Result<&FakeDeviceSpec> {
		if device.enumeration != self.generation {
			return Err(AudioError::DeviceNotFound);
		}
		self.specs
			.get(device.index as usize)
			.filter(|s| s.direction == direction)
			.ok_or(AudioError::DeviceNotFound)
	}

	fn actual(spec: &FakeDeviceSpec) -> ActualConfig {
		ActualConfig {
			sample_rate: spec.sample_rate,
			channels: spec.channels,
			format: spec.format,
			buffer_frames: None,
			max_callback_frames: spec.max_callback_frames,
		}
	}
}

impl AudioBackend for FakeBackend {
	fn enumerate(&mut self) -> Result<Enumeration> {
		self.generation += 1;
		let generation = self.generation;
		let devices = self
			.specs
			.iter()
			.enumerate()
			.map(|(index, spec)| DeviceDescriptor {
				handle: DeviceHandle {
					enumeration: generation,
					index: index as u32,
				},
				key: StableDeviceKey {
					host: "fake".to_owned(),
					direction: spec.direction,
					backend_id: spec.backend_id.clone(),
					name: spec.name.clone(),
					manufacturer: None,
					interface: None,
				},
				is_default: spec.is_default,
				configs: vec![ConfigRange {
					channels: spec.channels,
					min_rate: spec.sample_rate,
					max_rate: spec.sample_rate,
					format: Some(spec.format),
					max_buffer_frames: Some(spec.max_callback_frames),
				}],
			})
			.collect();
		Ok(Enumeration {
			generation,
			devices,
		})
	}

	fn open_capture(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> Result<CaptureStream> {
		let spec = self.spec(device, Direction::Input)?;
		if !request.select.is_valid_for(spec.channels) {
			return Err(AudioError::UnsupportedFormat(
				"channel selection out of range".into(),
			));
		}
		let config = Self::actual(spec);
		let (callback, reader, shared) = capture_pair(CaptureSetup {
			channels: config.channels,
			select: request.select,
			ring_samples: ring_samples_for(config.sample_rate, config.max_callback_frames),
		});
		let health = StreamHealth::default();
		let samples = config.max_callback_frames as usize * usize::from(config.channels);
		self.capture_driver = Some(FakeCaptureDriver {
			callback,
			config,
			health: health.clone(),
			signal: Signal::Silence,
			next_index: 0,
			f32_buf: vec![0.0; samples],
			i16_buf: vec![0; samples],
			unplugged: false,
		});
		Ok(CaptureStream {
			config,
			reader,
			shared,
			health,
			handle: Box::new(NoopHandle),
		})
	}

	fn open_playback(
		&mut self,
		device: DeviceHandle,
		_request: &StreamRequest,
	) -> Result<PlaybackStream> {
		let spec = self.spec(device, Direction::Output)?;
		let config = Self::actual(spec);
		let (callback, writer, shared) = playback_pair(
			config.channels,
			ring_samples_for(config.sample_rate, config.max_callback_frames),
		);
		let health = StreamHealth::default();
		let samples = config.max_callback_frames as usize * usize::from(config.channels);
		self.playback_driver = Some(FakePlaybackDriver {
			callback,
			config,
			health: health.clone(),
			f32_buf: vec![0.0; samples],
			i16_buf: vec![0; samples],
			recorded: Vec::new(),
			record_limit: 0,
			unplugged: false,
		});
		Ok(PlaybackStream {
			config,
			writer,
			shared,
			health,
			handle: Box::new(NoopHandle),
		})
	}
}
