//! The single-owner state machine.
//!
//! [`State::apply`] is the only way state changes. It takes an [`Input`] and
//! an explicit [`MonoTime`], and returns an [`Outcome`] with the result and
//! the effects adapters must perform. Nothing here reads a clock, touches a
//! socket, or allocates audio.

mod capture;
mod clients;
mod controller;
mod flush;
mod lifecycle;
mod owner;
mod peer;
mod routing;
mod watch;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use crate::call::{Call, Gain};
use crate::command::{Command, Input, Request};
use crate::effect::Effect;
use crate::error::{LimitsError, Reject};
use crate::event::{CaptureState, Event, EventLog, GlobalView, InputFault};
use crate::id::{
	AudioGeneration, CallId, ClientId, ControllerLeaseId, Incarnation, InputLeaseId, InputSourceId,
	OperationId, PeerId, Revision, SettingsRevision,
};
use crate::limits::{Limits, PTT_RELEASE_DELAY_MAX};
use crate::ops::{Lookup, OperationCache, OperationRecord};
use crate::outcome::{OpResult, Outcome, Reply};
use crate::time::MonoTime;

/// Engine-wide user controls. Mute is kept separate from deafen so that
/// undeafening never unmutes something the user muted explicitly.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Globals {
	pub muted: bool,
	pub deafened: bool,
	pub output_gain: Gain,
	pub ptt_release_delay: Duration,
	/// Mode a call starts in when it connects (see `SystemEvent::CallDefaults`).
	pub default_route: crate::call::RouteMode,
	pub settings: SettingsRevision,
}

/// OS session flags.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SessionFlags {
	pub locked: bool,
	pub suspended: bool,
}

impl SessionFlags {
	pub(crate) const fn allowed(self) -> bool {
		!self.locked && !self.suspended
	}
}

/// Shared capture path.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Capture {
	pub state: CaptureState,
	pub generation: AudioGeneration,
	pub exhausted: bool,
	/// The last open failed (the device could not be opened), as opposed to
	/// an open device being lost: the next call tries again.
	pub open_failed: bool,
}

/// An attached local client.
#[derive(Debug, Default)]
pub(crate) struct ClientSession {
	/// Last accepted sequence per input source.
	pub sources: BTreeMap<InputSourceId, u64>,
}

/// Per-apply accumulators, drained by `finish`.
#[derive(Debug, Default)]
pub(crate) struct Scratch {
	pub effects: Vec<Effect>,
	pub events: Vec<Event>,
	pub dirty_calls: BTreeSet<CallId>,
	pub dirty_global: bool,
}

/// The deterministic domain core.
#[derive(Debug)]
pub struct State {
	pub(crate) local: PeerId,
	pub(crate) incarnation: Incarnation,
	pub(crate) limits: Limits,
	pub(crate) revision: Revision,
	pub(crate) globals: Globals,
	pub(crate) session: SessionFlags,
	/// Clients holding an open watch; `None` while presence is not tracked.
	pub(crate) presence: Option<BTreeSet<ClientId>>,
	pub(crate) capture: Capture,
	pub(crate) clients: BTreeMap<ClientId, ClientSession>,
	pub(crate) links: BTreeSet<PeerId>,
	pub(crate) calls: BTreeMap<CallId, Call>,
	pub(crate) terminal_order: VecDeque<CallId>,
	pub(crate) aliases: BTreeMap<CallId, CallId>,
	pub(crate) alias_order: VecDeque<CallId>,
	pub(crate) ops: OperationCache,
	pub(crate) log: EventLog,
	pub(crate) next_controller: ControllerLeaseId,
	pub(crate) next_input: InputLeaseId,
	pub(crate) scratch: Scratch,
}

impl State {
	/// Creates an empty engine state. Fails if `limits` exceed the ceilings.
	pub fn new(
		local: PeerId,
		incarnation: Incarnation,
		limits: Limits,
	) -> Result<Self, LimitsError> {
		limits.validate()?;
		Ok(Self {
			local,
			incarnation,
			revision: Revision::ZERO,
			globals: Globals {
				muted: false,
				deafened: false,
				output_gain: Gain::UNITY,
				ptt_release_delay: limits.ptt_release_delay,
				default_route: crate::call::RouteMode::Muted,
				settings: SettingsRevision::ZERO,
			},
			session: SessionFlags::default(),
			presence: None,
			capture: Capture {
				state: CaptureState::Closed,
				generation: AudioGeneration::ZERO,
				exhausted: false,
				open_failed: false,
			},
			clients: BTreeMap::new(),
			links: BTreeSet::new(),
			calls: BTreeMap::new(),
			terminal_order: VecDeque::new(),
			aliases: BTreeMap::new(),
			alias_order: VecDeque::new(),
			ops: OperationCache::new(
				limits.operation_cache_per_client,
				limits.operation_cache_global,
				limits.operation_retention,
			),
			log: EventLog::new(limits.lifecycle_replay_events),
			next_controller: ControllerLeaseId::new(1),
			next_input: InputLeaseId::new(1),
			scratch: Scratch::default(),
			limits,
		})
	}

	/// The engine incarnation.
	pub fn incarnation(&self) -> Incarnation {
		self.incarnation
	}

	/// The current lifecycle revision.
	pub fn revision(&self) -> Revision {
		self.revision
	}

	/// The limits in force.
	pub fn limits(&self) -> &Limits {
		&self.limits
	}

	/// Looks up a call, resolving glare aliases.
	pub fn call(&self, id: CallId) -> Option<&Call> {
		self.calls.get(&self.resolve(id))
	}

	/// Number of calls that count against admission.
	pub fn live_calls(&self) -> usize {
		self.calls.values().filter(|call| call.is_live()).count()
	}

	/// Status of a previously processed operation, if still retained.
	pub fn operation(&self, client: ClientId, op: OperationId) -> Option<&OperationRecord> {
		self.ops.get(client, op)
	}

	/// Earliest instant at which `apply(Input::Tick, ..)` will change something.
	pub fn next_deadline(&self) -> Option<MonoTime> {
		let timers = self
			.calls
			.values()
			.filter_map(|call| call.deadline.map(|d| d.at));
		let leases = self
			.calls
			.values()
			.flat_map(|call| call.input_leases.values().map(|lease| lease.expires_at));
		let orphans = self.calls.values().filter_map(|call| call.orphan_at);
		timers.chain(leases).chain(orphans).min()
	}

	/// Processes one input at `now` and returns its outcome.
	pub fn apply(&mut self, input: Input, now: MonoTime) -> Outcome {
		self.ops.expire(now);
		self.expire_leases(now);
		self.run_timers(now);
		self.end_orphans(now);
		let mut duplicate = false;
		let result = match input {
			Input::Client(request) => {
				let (result, replayed) = self.apply_request(request, now);
				duplicate = replayed;
				result
			}
			Input::Disconnected(client) => {
				self.detach(client);
				Ok(Reply::Done)
			}
			Input::Peer(event) => self.on_peer(event, now),
			Input::System(event) => self.on_system(event),
			Input::Tick => Ok(Reply::Done),
		};
		self.reconcile_owners(now);
		self.finish(result, duplicate)
	}

	fn apply_request(&mut self, request: Request, now: MonoTime) -> (OpResult, bool) {
		let Request {
			client,
			operation,
			command,
		} = request;
		match self.ops.lookup(client, operation, &command) {
			Lookup::Replay(record) => return (record.result.clone(), true),
			Lookup::Conflict => return (Err(Reject::OperationIdReused), false),
			Lookup::Miss => {}
		}
		let result = self.execute(client, &command, now);
		let known = self.clients.contains_key(&client);
		if known && !matches!(command, Command::Detach) {
			self.ops
				.record(client, operation, command, result.clone(), now);
		}
		(result, false)
	}

	fn execute(&mut self, client: ClientId, command: &Command, now: MonoTime) -> OpResult {
		if matches!(command, Command::Attach) {
			return self.attach(client);
		}
		if !self.clients.contains_key(&client) {
			return Err(Reject::UnknownClient);
		}
		match *command {
			Command::Attach => Err(Reject::AlreadyAttached),
			Command::Detach => {
				self.detach(client);
				Ok(Reply::Done)
			}
			Command::StartCall {
				call,
				session,
				peer,
				allow_parallel_same_peer,
			} => self.start_call(client, call, session, peer, allow_parallel_same_peer, now),
			Command::AcceptCall { call, lease } => self.accept_call(client, call, lease, now),
			Command::DeclineCall { call, lease } => self.decline_call(client, call, lease, now),
			Command::EndCall { call, lease } => self.end_call(client, call, lease, now),
			Command::AcquireController { call, takeover } => {
				self.acquire_controller(client, call, takeover)
			}
			Command::ReleaseController { call, lease } => {
				self.release_controller(client, call, lease)
			}
			Command::SetTransmitMode { call, lease, route } => {
				self.set_route(client, call, lease, route)
			}
			Command::SetCallControls {
				call,
				lease,
				muted,
				deafened,
				gain,
			} => self.set_call_controls(client, call, lease, muted, deafened, gain),
			Command::SetGlobalControls {
				muted,
				deafened,
				output_gain,
				ptt_release_delay,
				expected,
			} => self.set_global_controls(muted, deafened, output_gain, ptt_release_delay, expected),
			Command::ResumeCapture => self.resume_capture(),
			Command::Ptt(update) => self.ptt(client, update, now),
		}
	}

	fn set_global_controls(
		&mut self,
		muted: Option<bool>,
		deafened: Option<bool>,
		output_gain: Option<Gain>,
		release_delay: Option<Duration>,
		expected: Option<SettingsRevision>,
	) -> OpResult {
		if release_delay.is_some_and(|delay| delay > PTT_RELEASE_DELAY_MAX) {
			return Err(Reject::InvalidReleaseDelay);
		}
		if let Some(expected) = expected
			&& expected != self.globals.settings
		{
			return Err(Reject::SettingsConflict {
				current: self.globals.settings,
			});
		}
		let before = (
			self.globals.muted,
			self.globals.deafened,
			self.globals.output_gain,
			self.globals.ptt_release_delay,
		);
		let g = &mut self.globals;
		g.muted = muted.unwrap_or(g.muted);
		g.deafened = deafened.unwrap_or(g.deafened);
		g.output_gain = output_gain.unwrap_or(g.output_gain);
		g.ptt_release_delay = release_delay.unwrap_or(g.ptt_release_delay);
		if before != (g.muted, g.deafened, g.output_gain, g.ptt_release_delay) {
			g.settings = g.settings.next().unwrap_or(g.settings);
			self.scratch.dirty_global = true;
		}
		// A mute or deafen is a deny: it ends any pending release hold.
		if g.muted || g.deafened {
			self.cancel_all_holds();
		}
		Ok(Reply::Done)
	}

	pub(crate) fn global_view(&self) -> GlobalView {
		GlobalView {
			muted: self.globals.muted,
			deafened: self.globals.deafened,
			output_gain: self.globals.output_gain,
			ptt_release_delay: self.globals.ptt_release_delay,
			settings: self.globals.settings,
			locked: self.session.locked,
			suspended: self.session.suspended,
			capture: self.capture.state,
			capture_generation: self.capture.generation,
		}
	}

	pub(crate) fn resolve(&self, id: CallId) -> CallId {
		self.aliases.get(&id).copied().unwrap_or(id)
	}

	pub(crate) fn push_event(&mut self, event: Event) {
		self.scratch.events.push(event);
	}

	pub(crate) fn push_effect(&mut self, effect: Effect) {
		self.scratch.effects.push(effect);
	}

	pub(crate) fn mark_dirty(&mut self, call: CallId) {
		self.scratch.dirty_calls.insert(call);
	}

	/// Removes every push-to-talk lease of a call and reports why.
	pub(crate) fn close_leases(&mut self, call: CallId, fault: InputFault) {
		let Some(entry) = self.calls.get_mut(&call) else {
			return;
		};
		if entry.input_leases.is_empty() {
			return;
		}
		entry.input_leases.clear();
		self.scratch.dirty_calls.insert(call);
		self.scratch.events.push(Event::InputFault { call, fault });
	}
}
