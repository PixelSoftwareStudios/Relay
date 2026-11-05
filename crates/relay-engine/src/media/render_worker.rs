//! The render worker: one thread owning every call's jitter buffer and
//! decoder, the mixer and the playback sink.
//!
//! It runs on a 10 ms schedule of its own. Each tick renders exactly one
//! frame per call, so a call that has nothing to play costs silence rather
//! than blocking another call.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use relay_audio::device::{Direction, Recovery, StreamHealth, recovery_for};
use relay_audio::mixer::{CallSlot, Mixer};
use relay_audio::playback::PlaybackShared;
use relay_audio::priority::WorkerKind;
use relay_audio::{FRAME_SAMPLES, MAX_CALLS};
use relay_core::{CallId, ReceiveMix};
use rtrb::Consumer;

use super::events::{EventSink, MediaEvent};
use super::load::LoadMeter;
use super::meters::{Meters, MonitorRx};
use super::packet::Ingress;
use super::recv::{CallReceiver, Rendered, SourceId};
use super::sink::RenderSink;
use super::supervise::{self, Supervised, Supervision};

const TICK: Duration = Duration::from_millis(10);
/// Missed ticks rendered back to back before the schedule resynchronizes.
const CATCH_UP_MAX: u32 = 4;
/// Packet queues the worker can hold without reallocating (one per peer).
const MAX_SOURCES: usize = MAX_CALLS;
/// Sleep with nothing to render; commands wake the worker sooner.
const IDLE_WAIT: Duration = Duration::from_millis(500);
/// Shortest sleep while ticking, so a late tick never busy-spins.
const MIN_WAIT: Duration = Duration::from_micros(200);

/// An opened playback stream handed to the worker.
pub struct RenderOutput {
	/// FIFO writer and drift loop.
	pub sink: RenderSink,
	/// Device health published by the stream's error callback.
	pub health: StreamHealth,
	/// Callback-side counters.
	pub shared: Arc<PlaybackShared>,
}

/// Commands accepted by the render worker.
pub enum RenderCmd {
	/// Starts rendering a call.
	AddCall(Box<CallReceiver>),
	/// Retires a call's receiver and decoder.
	RemoveCall(CallId),
	/// Registers a network task's packet queue.
	AddSource(SourceId, Consumer<Ingress>),
	/// Drops a packet queue.
	RemoveSource(SourceId),
	/// Updates a call's gain and audibility, or the master gain.
	Mix(ReceiveMix),
	/// Attaches an opened playback stream.
	Attach(Box<RenderOutput>),
	/// Detaches the playback stream.
	Detach,
}

struct Rendering {
	receiver: Box<CallReceiver>,
	slot: CallSlot,
	audible: bool,
}

struct Worker {
	commands: Consumer<RenderCmd>,
	events: EventSink,
	load: Arc<LoadMeter>,
	output: Option<Box<RenderOutput>>,
	calls: Vec<Rendering>,
	sources: Vec<(SourceId, Consumer<Ingress>)>,
	mixer: Mixer,
	frame: [f32; FRAME_SAMPLES],
	mixed: [f32; FRAME_SAMPLES],
	next_tick: Instant,
	meters: Arc<Meters>,
	monitor: MonitorRx,
	/// The call being rendered when a step panics.
	current: Option<CallId>,
}

/// Starts the render worker thread under `supervision`.
pub fn spawn(
	commands: Consumer<RenderCmd>,
	events: EventSink,
	load: Arc<LoadMeter>,
	meters: Arc<Meters>,
	monitor: MonitorRx,
	supervision: Supervision,
) -> std::io::Result<JoinHandle<()>> {
	let worker = Worker {
		commands,
		events,
		load,
		output: None,
		calls: Vec::with_capacity(MAX_CALLS),
		sources: Vec::with_capacity(MAX_SOURCES),
		mixer: Mixer::new(FRAME_SAMPLES),
		frame: [0.0; FRAME_SAMPLES],
		mixed: [0.0; FRAME_SAMPLES],
		next_tick: Instant::now(),
		meters,
		monitor,
		current: None,
	};
	thread::Builder::new()
		.name("relay-render".to_owned())
		.spawn(move || supervise::run(worker, &supervision, WorkerKind::Render))
}

impl Supervised for Worker {
	fn step(&mut self) {
		self.run_step();
	}

	fn wait(&self) -> Duration {
		if self.is_idle() {
			IDLE_WAIT
		} else {
			self.next_tick
				.saturating_duration_since(Instant::now())
				.max(MIN_WAIT)
		}
	}

	/// Removes the call whose receiver panicked, or every call when the
	/// culprit is unknown; the output stays attached when only a call failed.
	fn recover(&mut self) -> Option<CallId> {
		let culprit = self.current.take();
		match culprit {
			Some(call) => self.remove_call(call),
			None => {
				for call in std::mem::take(&mut self.calls) {
					self.mixer.close_slot(call.slot);
				}
			}
		}
		self.next_tick = Instant::now();
		culprit
	}

	fn disable(&mut self) {
		self.output = None;
		for call in std::mem::take(&mut self.calls) {
			self.mixer.close_slot(call.slot);
		}
		while self.commands.pop().is_ok() {}
	}
}

impl Worker {
	fn is_idle(&self) -> bool {
		self.output.is_none()
			|| (self.calls.is_empty() && !self.meters.monitor.load(Ordering::Relaxed))
	}

	fn remove_call(&mut self, call: CallId) {
		if let Some(i) = self.calls.iter().position(|c| c.receiver.call() == call) {
			let removed = self.calls.swap_remove(i);
			self.mixer.close_slot(removed.slot);
		}
	}

	fn run_step(&mut self) {
		self.drain_commands();
		self.drain_sources();
		if self.lost() {
			return;
		}
		if self.is_idle() {
			self.next_tick = Instant::now();
			if self.calls.is_empty() {
				self.load.reset();
			}
			return;
		}
		let now = Instant::now();
		let mut rendered = 0;
		while now >= self.next_tick && rendered < CATCH_UP_MAX {
			self.tick();
			self.next_tick += TICK;
			rendered += 1;
		}
		if now >= self.next_tick {
			// Far behind (suspend, debugger): skip ahead instead of bursting.
			self.next_tick = now + TICK;
		}
	}

	fn tick(&mut self) {
		let started = Instant::now();
		let Self {
			calls,
			mixer,
			frame,
			mixed,
			output,
			meters,
			monitor,
			current,
			..
		} = self;
		let Some(output) = output.as_mut() else {
			return;
		};
		mixer.begin();
		for call in calls.iter_mut() {
			*current = Some(call.receiver.call());
			let rendered = call.receiver.render(frame);
			if call.audible && rendered != Rendered::Silence {
				let _ = mixer.add(call.slot, frame);
			}
		}
		*current = None;
		if mixer.finish(mixed).is_ok() {
			if meters.monitor.load(Ordering::Relaxed) {
				mix_monitor(monitor, mixed);
			} else {
				while monitor.pop().is_ok() {}
			}
			let _ = output.sink.write(mixed);
		}
		meters
			.render_underruns
			.store(output.shared.underrun_frames(), Ordering::Relaxed);
		self.load
			.record(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
	}

	fn drain_sources(&mut self) {
		let Self { sources, calls, .. } = self;
		for (source, queue) in sources.iter_mut() {
			while let Ok(packet) = queue.pop() {
				// Packets are offered only to the call their sealed binding
				// names; the receiver re-checks every field.
				let wanted = packet.call;
				if let Some(call) = calls
					.iter_mut()
					.find(|c| c.receiver.call().to_bytes() == wanted)
				{
					call.receiver.ingest(*source, &packet);
				}
			}
		}
	}

	fn lost(&mut self) -> bool {
		let Some(output) = &self.output else {
			return false;
		};
		if recovery_for(Direction::Output, output.health.state()) == Recovery::Continue {
			return false;
		}
		self.output = None;
		(self.events)(MediaEvent::OutputLost);
		true
	}

	fn drain_commands(&mut self) {
		while let Ok(command) = self.commands.pop() {
			match command {
				RenderCmd::AddCall(receiver) => {
					if let Some(slot) = self.mixer.open_slot() {
						self.calls.push(Rendering {
							receiver,
							slot,
							audible: true,
						});
					} else {
						tracing::warn!("mixer is full; call not rendered");
					}
				}
				RenderCmd::RemoveCall(call) => self.remove_call(call),
				RenderCmd::AddSource(id, queue) => {
					if self.sources.len() < MAX_SOURCES {
						self.sources.push((id, queue));
					} else {
						tracing::warn!("render worker has no room for another source");
					}
				}
				RenderCmd::RemoveSource(id) => self.sources.retain(|(s, _)| *s != id),
				RenderCmd::Mix(mix) => self.apply_mix(mix),
				RenderCmd::Attach(output) => {
					self.output = Some(output);
					self.next_tick = Instant::now();
				}
				RenderCmd::Detach => self.output = None,
			}
		}
	}

	fn apply_mix(&mut self, mix: ReceiveMix) {
		let permille = |g: relay_core::Gain| f32::from(g.permille()) / 1000.0;
		let _ = self.mixer.set_master_gain(permille(mix.master));
		if let Some(call) = self
			.calls
			.iter_mut()
			.find(|c| c.receiver.call() == mix.call)
		{
			call.audible = mix.audible;
			let _ = self.mixer.set_call_gain(call.slot, permille(mix.gain));
		}
	}
}

/// Adds one queued monitor frame to the mix. Anything older is discarded, so
/// the local echo never lags behind by more than a frame.
fn mix_monitor(monitor: &mut MonitorRx, mixed: &mut [f32; FRAME_SAMPLES]) {
	let mut latest = None;
	while let Ok(frame) = monitor.pop() {
		latest = Some(frame);
	}
	if let Some(frame) = latest {
		for (out, sample) in mixed.iter_mut().zip(frame) {
			*out = (*out + sample).clamp(-1.0, 1.0);
		}
	}
}
