//! The device thread: the only owner of the audio backend and its stream
//! handles, which may not be `Send` on every platform.
//!
//! It opens and closes streams on request and hands the reader and writer
//! halves to the workers. Selection is explicit: a missing device is a
//! failure the user must resolve, never a reason to pick another.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

use arc_swap::ArcSwap;
use relay_audio::AudioError;
use relay_audio::capture::CaptureChain;
use relay_audio::device::{
	AudioBackend, DeviceDescriptor, DeviceSelection, Direction, Resolution, StreamHandle,
	StreamRequest,
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

/// The device selection in force. There is no "automatic" input: `None`
/// means the user has not chosen a microphone yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
	/// Microphone choice.
	pub input: Option<DeviceSelection>,
	/// Output choice.
	pub output: DeviceSelection,
}

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
		while let Ok(request) = requests.recv() {
			match request {
				DeviceCmd::OpenCapture(generation) => self.open_capture(generation),
				DeviceCmd::CloseCapture(generation) => self.close_capture(generation),
				DeviceCmd::OpenPlayback(reply) => {
					let _ = reply.send(self.open_playback().map_err(|e| e.to_string()));
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
		if let Ok(enumeration) = self.backend.enumerate() {
			self.listing.store(Arc::new(enumeration.devices));
		}
	}

	fn emit(&self, event: SystemEvent) {
		(self.events)(MediaEvent::System(event));
	}

	fn open_capture(&mut self, generation: AudioGeneration) {
		self.close_current_capture();
		match self.try_open_capture(generation) {
			Ok(handle) => {
				self.open_capture = Some((generation, handle));
				self.emit(SystemEvent::CaptureReady { generation });
			}
			Err(error) => {
				tracing::warn!(%error, "capture open failed");
				self.emit(SystemEvent::CaptureFailed { generation });
			}
		}
	}

	fn try_open_capture(
		&mut self,
		generation: AudioGeneration,
	) -> Result<Box<dyn StreamHandle>, AudioError> {
		let selection = self
			.setup
			.selection
			.input
			.clone()
			.ok_or(AudioError::DeviceNotFound)?;
		let device = self.resolve(Direction::Input, &selection)?;
		let stream = self.backend.open_capture(device, &self.setup.request)?;
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
		let selection = self.setup.selection.output.clone();
		let device = self.resolve(Direction::Output, &selection)?;
		let stream = self.backend.open_playback(device, &self.setup.request)?;
		let sink = RenderSink::new(
			stream.writer,
			stream.config.sample_rate,
			self.setup.playout_target_ms,
		)?;
		stream.handle.play()?;
		let output = Box::new(RenderOutput {
			sink,
			health: stream.health,
		});
		self.render
			.push(RenderCmd::Attach(output))
			.map_err(|_| AudioError::InvalidArgument("render worker queue is full"))?;
		self.open_playback = Some(stream.handle);
		Ok(())
	}

	fn close_playback(&mut self) {
		if let Some(handle) = self.open_playback.take() {
			let _ = handle.pause();
			let _ = self.render.push(RenderCmd::Detach);
		}
	}

	fn resolve(
		&mut self,
		direction: Direction,
		selection: &DeviceSelection,
	) -> Result<relay_audio::device::DeviceHandle, AudioError> {
		let enumeration = self.backend.enumerate()?;
		let resolved = enumeration.resolve(direction, selection);
		self.listing.store(Arc::new(enumeration.devices));
		match resolved {
			Resolution::Device(handle) => Ok(handle),
			Resolution::NoDefault => Err(AudioError::DeviceNotFound),
			Resolution::PinnedMissing => Err(AudioError::SelectedDeviceMissing),
		}
	}
}
