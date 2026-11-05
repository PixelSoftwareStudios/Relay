//! The capture worker: one thread owning the shared microphone chain, the
//! noise suppressor and every call's encoder.
//!
//! Frames flow capture reader -> suppressor -> frame assembler -> one encoder
//! per call. Each call's gate is consulted by its sender immediately before
//! every packet; a closed or stale gate drops the frame at that call only.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use relay_audio::capture::{CaptureChain, CaptureShared};
use relay_audio::device::{Direction, Recovery, StreamHealth, recovery_for};
use relay_audio::dsp::{Discontinuity, NoiseSuppressor};
use relay_audio::{FRAME_SAMPLES, MAX_CALLS};
use relay_core::{AudioGeneration, CallId, SystemEvent};
use relay_protocol::assembly::FrameAssembler;
use rtrb::Consumer;

use super::events::{EventSink, MediaEvent};
use super::gate::FrameStamp;
use super::load::LoadMeter;
use super::send::CallSender;

const IDLE_WAIT: Duration = Duration::from_millis(2);

/// An opened microphone stream handed to the worker.
pub struct CaptureInput {
	/// Reader, resampler and framing for the device.
	pub chain: CaptureChain,
	/// The generation this stream was opened under.
	pub generation: AudioGeneration,
	/// Device health published by the stream's error callback.
	pub health: StreamHealth,
	/// Callback-side counters and the mute flag.
	pub shared: Arc<CaptureShared>,
}

/// Commands accepted by the capture worker.
pub enum CaptureCmd {
	/// Starts encoding for a call (the sender stays closed until its gate opens).
	AddSender(Box<CallSender>),
	/// Retires a call's sender and encoder.
	RemoveSender(CallId),
	/// Attaches an opened microphone stream.
	Attach(Box<CaptureInput>),
	/// Detaches the stream opened under this generation.
	Detach(AudioGeneration),
	/// Replaces the noise suppressor (already prepared).
	SetSuppressor(Box<dyn NoiseSuppressor>),
	/// Sets the microphone gain applied after the suppressor.
	SetGain(f32),
}

struct Worker {
	commands: Consumer<CaptureCmd>,
	events: EventSink,
	load: Arc<LoadMeter>,
	input: Option<Box<CaptureInput>>,
	senders: Vec<CallSender>,
	stamps: Vec<FrameStamp>,
	suppressor: Box<dyn NoiseSuppressor>,
	assembler: FrameAssembler<f32>,
	denoised: Vec<f32>,
	frame: [f32; FRAME_SAMPLES],
	timeline: Timeline,
	gain: f32,
}

/// Tracks the capture sample index of the frame being encoded.
#[derive(Default)]
struct Timeline {
	generation: Option<u64>,
	next_sample: u64,
}

/// Starts the capture worker thread.
pub fn spawn(
	commands: Consumer<CaptureCmd>,
	suppressor: Box<dyn NoiseSuppressor>,
	events: EventSink,
	load: Arc<LoadMeter>,
	stop: Arc<AtomicBool>,
) -> std::io::Result<JoinHandle<()>> {
	let mut worker = Worker {
		commands,
		events,
		load,
		input: None,
		senders: Vec::with_capacity(MAX_CALLS),
		stamps: Vec::with_capacity(MAX_CALLS),
		suppressor,
		assembler: FrameAssembler::new(FRAME_SAMPLES, FRAME_SAMPLES),
		denoised: vec![0.0; FRAME_SAMPLES * 2],
		frame: [0.0; FRAME_SAMPLES],
		timeline: Timeline::default(),
		gain: 1.0,
	};
	thread::Builder::new()
		.name("relay-capture".to_owned())
		.spawn(move || {
			while !stop.load(Ordering::Acquire) {
				worker.step();
				thread::park_timeout(IDLE_WAIT);
			}
		})
}

impl Worker {
	fn step(&mut self) {
		self.drain_commands();
		if self.lost() {
			return;
		}
		let started = Instant::now();
		let Self {
			input,
			senders,
			stamps,
			suppressor,
			assembler,
			denoised,
			frame,
			timeline,
			gain,
			..
		} = self;
		let Some(input) = input.as_mut() else {
			return;
		};
		// Stamp once per poll, before any frame of the batch is processed:
		// a route change that lands mid-batch makes the rest of it stale.
		stamps.clear();
		stamps.extend(senders.iter().map(|s| s.stamp(input.generation)));
		let polled = input.chain.poll(|raw, timing| {
			if timeline.generation != Some(timing.generation) {
				timeline.generation = Some(timing.generation);
				timeline.next_sample = timing.first_sample;
				assembler.clear();
				suppressor.reset(Discontinuity::Reset);
			}
			let Ok(progress) = suppressor.process_into(raw, denoised, timing) else {
				return;
			};
			let produced = progress.produced.min(denoised.len());
			apply_gain(&mut denoised[..produced], *gain);
			if assembler.push(&denoised[..produced]).is_err() {
				assembler.clear();
				return;
			}
			while assembler.pop_frame(frame) {
				for (sender, stamp) in senders.iter_mut().zip(stamps.iter()) {
					sender.emit(frame, *stamp, timeline.next_sample);
				}
				timeline.next_sample += FRAME_SAMPLES as u64;
			}
		});
		if let Ok(stats) = polled
			&& stats.frames_emitted > 0
		{
			let ns = started.elapsed().as_nanos() / stats.frames_emitted as u128;
			self.load.record(u64::try_from(ns).unwrap_or(u64::MAX));
		}
	}

	/// Pauses on device loss and tells the owner; nothing is substituted.
	fn lost(&mut self) -> bool {
		let Some(input) = &self.input else {
			return false;
		};
		if recovery_for(Direction::Input, input.health.state()) == Recovery::Continue {
			return false;
		}
		let generation = input.generation;
		self.input = None;
		(self.events)(MediaEvent::System(SystemEvent::CaptureLost { generation }));
		true
	}

	fn drain_commands(&mut self) {
		while let Ok(command) = self.commands.pop() {
			match command {
				CaptureCmd::AddSender(sender) => {
					if self.senders.len() < MAX_CALLS {
						self.senders.push(*sender);
					}
				}
				CaptureCmd::RemoveSender(call) => self.senders.retain(|s| s.call() != call),
				CaptureCmd::Attach(input) => {
					self.timeline = Timeline::default();
					self.assembler.clear();
					self.input = Some(input);
				}
				CaptureCmd::Detach(generation) => {
					if self
						.input
						.as_ref()
						.is_some_and(|i| i.generation == generation)
					{
						self.input = None;
					}
				}
				CaptureCmd::SetSuppressor(suppressor) => self.suppressor = suppressor,
				CaptureCmd::SetGain(gain) => self.gain = gain,
			}
		}
	}
}

/// Scales samples and keeps them inside the representable range.
fn apply_gain(samples: &mut [f32], gain: f32) {
	if (gain - 1.0).abs() < f32::EPSILON {
		return;
	}
	for sample in samples {
		*sample = (*sample * gain).clamp(-1.0, 1.0);
	}
}
