//! The capture worker: one thread owning the shared microphone chain, the
//! noise suppressor and every call's encoder.
//!
//! Frames flow capture reader -> suppressor -> frame assembler -> one encoder
//! per call. Each call's gate is consulted by its sender immediately before
//! every packet; a closed or stale gate drops the frame at that call only.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use relay_audio::priority::WorkerKind;

use relay_audio::capture::{CaptureChain, CaptureShared};
use relay_audio::device::{Direction, Recovery, StreamHealth, recovery_for};
use relay_audio::dsp::{Discontinuity, DspLimits, NoiseSuppressor, OffSuppressor};
use relay_audio::{BlockTiming, FRAME_SAMPLES, MAX_CALLS};
use relay_core::{AudioGeneration, CallId, SystemEvent};
use relay_protocol::assembly::FrameAssembler;
use rtrb::Consumer;

use super::events::{EventSink, MediaEvent};
use super::gate::FrameStamp;
use super::load::LoadMeter;
use super::meters::{Meters, MonitorTx, level_dbfs};
use super::send::CallSender;
use super::supervise::{self, Supervised, Supervision};

/// Poll period while a microphone is attached.
const POLL_WAIT: Duration = Duration::from_millis(2);
/// Sleep with nothing attached; commands wake the worker sooner.
const IDLE_WAIT: Duration = Duration::from_millis(500);
/// A poll later than this after the previous one may be draining a backlog of
/// unknown age; its audio is not sent.
const STALL: Duration = Duration::from_millis(20);

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
	meters: Arc<Meters>,
	monitor: MonitorTx,
	last_poll: Option<Instant>,
}

/// Tracks the capture sample index of the frame being encoded.
#[derive(Default)]
struct Timeline {
	generation: Option<u64>,
	next_sample: u64,
}

/// Starts the capture worker thread under `supervision`.
pub fn spawn(
	commands: Consumer<CaptureCmd>,
	suppressor: Box<dyn NoiseSuppressor>,
	events: EventSink,
	load: Arc<LoadMeter>,
	meters: Arc<Meters>,
	monitor: MonitorTx,
	supervision: Supervision,
) -> std::io::Result<JoinHandle<()>> {
	let worker = Worker {
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
		meters,
		monitor,
		last_poll: None,
	};
	thread::Builder::new()
		.name("relay-capture".to_owned())
		.spawn(move || supervise::run(worker, &supervision, WorkerKind::Capture))
}

impl Supervised for Worker {
	fn step(&mut self) {
		self.run_step();
	}

	fn wait(&self) -> Duration {
		if self.input.is_some() {
			POLL_WAIT
		} else {
			IDLE_WAIT
		}
	}

	/// Drops the stream and the suspect suppressor, and tells the domain the
	/// microphone is gone: it closes every gate and the device. Senders are
	/// disarmed, so nothing is sent until a fresh stream has been polled.
	fn recover(&mut self) -> Option<CallId> {
		let generation = self.input.take().map(|i| i.generation);
		self.meters.microphone.clear();
		self.assembler.clear();
		self.timeline = Timeline::default();
		self.last_poll = None;
		self.senders.iter_mut().for_each(CallSender::rearm);
		let mut fallback: Box<dyn NoiseSuppressor> = Box::new(OffSuppressor::default());
		let limits = DspLimits {
			max_block_samples: FRAME_SAMPLES,
			max_scratch_bytes: 0,
		};
		if fallback.prepare(&limits).is_ok() {
			self.suppressor = fallback;
		}
		if let Some(generation) = generation {
			(self.events)(MediaEvent::System(SystemEvent::CaptureLost { generation }));
		}
		None
	}

	fn disable(&mut self) {
		self.input = None;
		self.senders.clear();
		while self.commands.pop().is_ok() {}
	}
}

impl Worker {
	fn run_step(&mut self) {
		self.drain_commands();
		if self.lost() {
			return;
		}
		let started = Instant::now();
		let stalled = self
			.last_poll
			.is_some_and(|t| started.duration_since(t) > STALL);
		self.last_poll = Some(started);
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
			meters,
			monitor,
			..
		} = self;
		let Some(input) = input.as_mut() else {
			meters.microphone.clear();
			self.last_poll = None;
			self.load.reset();
			return;
		};
		// Stamp once per poll, before any frame of the batch is processed:
		// a route change that lands mid-batch makes the rest of it stale, and
		// a gate that was not open for the whole poll interval makes the
		// batch's audio unsendable (it may predate the gate opening).
		stamps.clear();
		stamps.extend(
			senders
				.iter_mut()
				.map(|s| s.stamp(input.generation, stalled)),
		);
		let polled = input.chain.poll(|raw, timing| {
			if timeline.generation != Some(timing.generation) {
				timeline.generation = Some(timing.generation);
				timeline.next_sample = timing.first_sample;
				assembler.clear();
				suppressor.reset(Discontinuity::Reset);
				senders.iter_mut().for_each(CallSender::reset_timeline);
			}
			let mut offset = 0;
			while offset < raw.len() {
				let at = BlockTiming {
					first_sample: timing.first_sample + offset as u64,
					..timing
				};
				let Ok(progress) = suppressor.process_into(&raw[offset..], denoised, at) else {
					// Count the excision and keep the timeline exact, so the
					// receiver sees a gap rather than silently shifted audio.
					meters.suppressor_errors.fetch_add(1, Ordering::Relaxed);
					assembler.clear();
					timeline.next_sample = timing.first_sample + raw.len() as u64;
					return;
				};
				let produced = progress.produced.min(denoised.len());
				apply_gain(&mut denoised[..produced], *gain);
				if produced > 0 {
					meters.microphone.set(level_dbfs(&denoised[..produced]));
				}
				if assembler.push(&denoised[..produced]).is_err() {
					assembler.clear();
					return;
				}
				while assembler.pop_frame(frame) {
					if meters.monitor.load(Ordering::Relaxed) {
						// A full queue means the speaker side is behind: drop, never wait.
						let _ = monitor.push(*frame);
					}
					for (sender, stamp) in senders.iter_mut().zip(stamps.iter()) {
						sender.emit(frame, *stamp, timeline.next_sample);
					}
					timeline.next_sample += FRAME_SAMPLES as u64;
				}
				if progress.consumed == 0 {
					break;
				}
				offset += progress.consumed;
			}
		});
		let delay = input.chain.delay_samples()
			+ suppressor
				.descriptor()
				.declared_delay_samples
				.unwrap_or_default();
		let read_end = input.chain.read_end();
		for (sender, stamp) in senders.iter_mut().zip(stamps.iter()) {
			sender.end_poll(*stamp, read_end, delay as u64);
		}
		meters
			.capture_overruns
			.store(input.shared.overrun_frames(), Ordering::Relaxed);
		if let Ok(stats) = polled
			&& stats.frames_emitted > 0
		{
			let ns = started.elapsed().as_nanos() / stats.frames_emitted as u128;
			self.load.record(u64::try_from(ns).unwrap_or(u64::MAX));
		}
		if self.senders.is_empty() {
			self.load.reset();
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
		self.meters.microphone.clear();
		(self.events)(MediaEvent::System(SystemEvent::CaptureLost { generation }));
		true
	}

	fn drain_commands(&mut self) {
		while let Ok(command) = self.commands.pop() {
			match command {
				CaptureCmd::AddSender(sender) => {
					if self.senders.len() < MAX_CALLS {
						let mut sender = *sender;
						sender.rearm();
						self.senders.push(sender);
					} else {
						tracing::warn!("capture worker is full; sender refused");
					}
				}
				CaptureCmd::RemoveSender(call) => self.senders.retain(|s| s.call() != call),
				CaptureCmd::Attach(input) => {
					self.timeline = Timeline::default();
					self.assembler.clear();
					self.last_poll = None;
					self.senders.iter_mut().for_each(CallSender::rearm);
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
