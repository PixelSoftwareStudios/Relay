//! The device thread: the only owner of the audio backend and its stream
//! handles, which may not be `Send` on every platform.
//!
//! It opens and closes streams on request and hands the reader and writer
//! halves to the workers. Selection is explicit: a missing device is a
//! failure the user must resolve, never a reason to pick another.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use relay_audio::AudioError;
use relay_audio::capture::CaptureChain;
use relay_audio::device::{
	AudioBackend, DeviceDescriptor, DeviceSelection, Direction, Outcome, Poll, Reconnect,
	Resolution, StreamHandle, StreamHealth, StreamRequest, StreamState,
};
use relay_core::{AudioGeneration, SystemEvent};
use tokio::sync::oneshot;

use super::capture_worker::{CaptureCmd, CaptureInput};
use super::events::{EventSink, MediaEvent};
use super::inbox::Inbox;
use super::render_worker::{RenderCmd, RenderOutput};
use super::sink::RenderSink;

/// Builds the backend on the device thread.
pub type BackendFactory = Box<dyn FnOnce() -> Box<dyn AudioBackend> + Send>;

/// The device selection in force. `FollowDefault` is the starting point for
/// both directions: nothing has to be chosen for audio to work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
	/// Microphone choice.
	pub input: DeviceSelection,
	/// Output choice.
	pub output: DeviceSelection,
}

impl Default for Selection {
	fn default() -> Self {
		Self {
			input: DeviceSelection::FollowDefault,
			output: DeviceSelection::FollowDefault,
		}
	}
}

/// How often the device thread re-scans the hardware while idle, so a
/// plugged-in headset or a new system default shows up in the device list.
const RESCAN_EVERY: Duration = Duration::from_secs(2);
/// How often an open playback stream's health is looked at: a stream the
/// platform disconnected is reopened within this much plus the first backoff.
const HEALTH_EVERY: Duration = Duration::from_millis(20);

/// Requests to the device thread.
pub enum DeviceCmd {
	/// Opens the microphone under `generation`; completes with
	/// `CaptureReady` or `CaptureFailed`.
	OpenCapture(AudioGeneration),
	/// Closes the microphone opened under `generation`.
	CloseCapture(AudioGeneration),
	/// Opens the playback device if it is not open.
	OpenPlayback(oneshot::Sender<Result<(), String>>),
	/// Closes the playback device.
	ClosePlayback,
	/// Changes the selection and refreshes the device list.
	Select(Box<Selection>),
	/// Re-enumerates devices.
	Refresh,
	/// Stops the thread.
	Shutdown,
}

/// Static device-thread settings.
pub struct DeviceSetup {
	/// Stream parameters requested from the backend.
	pub request: StreamRequest,
	/// Initial selection.
	pub selection: Selection,
	/// FIFO fill the drift loop steers toward, in milliseconds.
	pub playout_target_ms: u32,
	/// Where the open streams' buffer sizes are published for diagnostics.
	pub buffers: Arc<super::diag::DeviceBuffers>,
}

struct Thread {
	backend: Box<dyn AudioBackend>,
	setup: DeviceSetup,
	capture: Inbox<CaptureCmd>,
	render: Inbox<RenderCmd>,
	events: EventSink,
	listing: Arc<ArcSwap<Vec<DeviceDescriptor>>>,
	open_capture: Option<(AudioGeneration, Box<dyn StreamHandle>)>,
	open_playback: Option<Box<dyn StreamHandle>>,
	/// Health of the open playback stream, shared with the render worker.
	playback_health: Option<StreamHealth>,
	/// Recovery of the playback stream after the platform disconnected it.
	playback_recovery: Reconnect,
	started: Instant,
	last_scan: Instant,
}

/// A running device thread.
pub struct DeviceThread {
	/// Request channel.
	pub commands: Sender<DeviceCmd>,
	/// The latest enumeration.
	pub listing: Arc<ArcSwap<Vec<DeviceDescriptor>>>,
	join: Option<JoinHandle<()>>,
}

impl DeviceThread {
	/// Starts the thread; the backend is built on it.
	pub fn spawn(
		factory: BackendFactory,
		setup: DeviceSetup,
		capture: Inbox<CaptureCmd>,
		render: Inbox<RenderCmd>,
		events: EventSink,
	) -> std::io::Result<Self> {
		let (commands, requests) = channel();
		let listing = Arc::new(ArcSwap::from_pointee(Vec::new()));
		let shared = Arc::clone(&listing);
		let join = thread::Builder::new()
			.name("relay-devices".to_owned())
			.spawn(move || {
				let mut thread = Thread {
					backend: factory(),
					setup,
					capture,
					render,
					events,
					listing: shared,
					open_capture: None,
					open_playback: None,
					playback_health: None,
					playback_recovery: Reconnect::new(),
					started: Instant::now(),
					last_scan: Instant::now(),
				};
				thread.refresh();
				thread.run(&requests);
			})?;
		Ok(Self {
			commands,
			listing,
			join: Some(join),
		})
	}
}

impl Drop for DeviceThread {
	fn drop(&mut self) {
		let _ = self.commands.send(DeviceCmd::Shutdown);
		if let Some(join) = self.join.take() {
			let _ = join.join();
		}
	}
}

impl Thread {
	fn run(&mut self, requests: &Receiver<DeviceCmd>) {
		loop {
			let wait = if self.playback_health.is_some() {
				HEALTH_EVERY
			} else {
				RESCAN_EVERY
			};
			let request = match requests.recv_timeout(wait) {
				Ok(request) => request,
				Err(RecvTimeoutError::Timeout) => {
					self.keep_playback_alive();
					if self.last_scan.elapsed() >= RESCAN_EVERY {
						self.refresh();
					}
					continue;
				}
				Err(RecvTimeoutError::Disconnected) => break,
			};
			match request {
				DeviceCmd::OpenCapture(generation) => self.open_capture(generation),
				DeviceCmd::CloseCapture(generation) => self.close_capture(generation),
				DeviceCmd::OpenPlayback(reply) => {
					let opened = self.open_playback();
					if opened.is_ok() {
						self.playback_opened_by_request();
					}
					let _ = reply.send(opened.map_err(|e| e.to_string()));
				}
				DeviceCmd::ClosePlayback => self.close_playback(),
				DeviceCmd::Select(selection) => {
					self.setup.selection = *selection;
					self.refresh();
				}
				DeviceCmd::Refresh => self.refresh(),
				DeviceCmd::Shutdown => break,
			}
		}
		self.close_playback();
		if let Some((generation, _)) = self.open_capture.take() {
			let _ = self.capture.push(CaptureCmd::Detach(generation));
		}
	}

	fn refresh(&mut self) {
		self.last_scan = Instant::now();
		if let Ok(enumeration) = self.backend.enumerate() {
			self.listing.store(Arc::new(enumeration.devices));
		}
	}

	fn events_out(&self, event: MediaEvent) {
		(self.events)(event);
	}

	fn emit(&self, event: SystemEvent) {
		(self.events)(MediaEvent::System(event));
	}

	fn open_capture(&mut self, generation: AudioGeneration) {
		self.close_current_capture();
		match self.try_open_capture(generation) {
			Ok(handle) => {
				self.open_capture = Some((generation, handle));
				self.events_out(MediaEvent::CaptureOpened);
				self.emit(SystemEvent::CaptureReady { generation });
			}
			Err(error) => {
				tracing::warn!(%error, "capture open failed");
				self.events_out(MediaEvent::CaptureUnavailable(error.to_string()));
				self.emit(SystemEvent::CaptureFailed { generation });
			}
		}
	}

	fn try_open_capture(
		&mut self,
		generation: AudioGeneration,
	) -> Result<Box<dyn StreamHandle>, AudioError> {
		let device = self.resolve(Direction::Input)?;
		let stream = self.backend.open_capture(device, &self.setup.request)?;
		self.setup.buffers.set_capture(
			stream
				.config
				.callback_frames
				.or(stream.config.buffer_frames)
				.unwrap_or(0),
			stream.config.sample_rate,
		);
		let chain = CaptureChain::new(stream.reader, stream.config.sample_rate)?;
		stream.handle.play()?;
		let input = Box::new(CaptureInput {
			chain,
			generation,
			health: stream.health,
			shared: stream.shared,
		});
		self.capture
			.push(CaptureCmd::Attach(input))
			.map_err(|_| AudioError::InvalidArgument("capture worker queue is full"))?;
		Ok(stream.handle)
	}

	fn close_current_capture(&mut self) {
		if let Some((generation, handle)) = self.open_capture.take() {
			let _ = handle.pause();
			let _ = self.capture.push(CaptureCmd::Detach(generation));
		}
	}

	fn close_capture(&mut self, generation: AudioGeneration) {
		if self
			.open_capture
			.as_ref()
			.is_some_and(|(g, _)| *g == generation)
		{
			self.close_current_capture();
		}
	}

	fn open_playback(&mut self) -> Result<(), AudioError> {
		if self.open_playback.is_some() {
			return Ok(());
		}
		let device = self.resolve(Direction::Output)?;
		let stream = self.backend.open_playback(device, &self.setup.request)?;
		tracing::info!(
			rate = stream.config.sample_rate,
			buffer_frames = ?stream.config.buffer_frames,
			callback_frames = ?stream.config.callback_frames,
			"playback opened"
		);
		self.setup.buffers.set_playback(
			stream
				.config
				.callback_frames
				.or(stream.config.buffer_frames)
				.unwrap_or(0),
			stream.config.sample_rate,
		);
		let mut sink = RenderSink::new(
			stream.writer,
			stream.config.sample_rate,
			self.setup.playout_target_ms,
		)?
		.track_callbacks(Arc::clone(&stream.shared))
		.with_callback_hint(stream.config.callback_frames);
		// Fill the FIFO with silence and hand the worker its output before the
		// device starts pulling: the first callbacks then have data and the
		// first tone sample is never late.
		sink.prefill();
		let health = stream.health.clone();
		let output = Box::new(RenderOutput {
			sink,
			health: stream.health,
			shared: Arc::clone(&stream.shared),
		});
		self.render
			.push(RenderCmd::Attach(output))
			.map_err(|_| AudioError::InvalidArgument("render worker queue is full"))?;
		stream.handle.play()?;
		self.open_playback = Some(stream.handle);
		self.playback_health = Some(health);
		Ok(())
	}

	fn close_playback(&mut self) {
		self.playback_health = None;
		self.playback_recovery = Reconnect::new();
		if let Some(handle) = self.open_playback.take() {
			let _ = handle.pause();
			let _ = self.render.push(RenderCmd::Detach);
		}
	}

	/// An explicit open succeeded: it ends any outage being worked on.
	fn playback_opened_by_request(&mut self) {
		if self.playback_recovery.warned() {
			self.events_out(MediaEvent::OutputRestored);
		}
		self.playback_recovery = Reconnect::new();
	}

	/// Reopens the playback stream when the platform disconnected it. A route
	/// change (call mode, headset, Bluetooth) does that during every call and
	/// is no reason to tell anyone; the user hears about an output that stays
	/// gone, once, and about it coming back.
	fn keep_playback_alive(&mut self) {
		let Some(health) = &self.playback_health else {
			return;
		};
		let now = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
		let healthy = health.state() == StreamState::Running;
		if self.playback_recovery.poll(now, healthy) != Poll::Reopen {
			return;
		}
		if let Some(handle) = self.open_playback.take() {
			let _ = handle.pause();
		}
		let reopened = self.open_playback();
		let now = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
		if let Err(error) = &reopened {
			tracing::warn!(%error, "playback reopen failed");
		}
		match self.playback_recovery.attempted(now, reopened.is_ok()) {
			Outcome::Restored { was_warned } => {
				tracing::info!("playback reopened on the new route");
				if was_warned {
					self.events_out(MediaEvent::OutputRestored);
				}
			}
			Outcome::Retry => {}
			Outcome::Warn => self.events_out(MediaEvent::OutputLost),
		}
	}

	fn resolve(
		&mut self,
		direction: Direction,
	) -> Result<relay_audio::device::DeviceHandle, AudioError> {
		let enumeration = self.backend.enumerate()?;
		let selection = match direction {
			Direction::Input => &self.setup.selection.input,
			Direction::Output => &self.setup.selection.output,
		};
		let resolved = enumeration.resolve(direction, selection);
		let fallback = enumeration.resolve(direction, &DeviceSelection::FollowDefault);
		self.listing.store(Arc::new(enumeration.devices));
		match resolved {
			Resolution::Device(handle) => Ok(handle),
			Resolution::NoDefault => Err(AudioError::DeviceNotFound),
			Resolution::PinnedMissing => {
				// The pinned device is gone: use the system default and let the
				// state say so (`*_pinned_missing`); never pick any other device.
				tracing::warn!(
					?direction,
					"pinned audio device is missing; using the system default"
				);
				match fallback {
					Resolution::Device(handle) => Ok(handle),
					_ => Err(AudioError::SelectedDeviceMissing),
				}
			}
		}
	}
}
