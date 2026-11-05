//! Randomized traces: whatever order things happen in, privacy invariants
//! and the no-gap watch contract must hold after every single step.

mod common;

use std::collections::BTreeMap;

use proptest::prelude::*;
use relay_core::testing::{FakeWatcher, Node, device_key};
use relay_core::*;

#[derive(Debug, Clone)]
enum Step {
	Attach(u8),
	Disconnect(u8),
	Start(u8, u8, bool),
	Invite(u8),
	Acquire(u8, u8, bool),
	Release(u8, u8),
	Accept(u8, u8),
	Decline(u8, u8),
	End(u8, u8),
	Route(u8, u8, u8),
	Ptt(u8, u8, u8, bool, bool),
	CallControls(u8, u8, bool, bool),
	Global(bool, bool),
	LinkUp(u8),
	LinkDown(u8),
	RemoteAccept(u8),
	RemoteEnd(u8),
	MediaReady(u8),
	MediaStopped(u8),
	CaptureReady,
	CaptureStale,
	Unplug,
	Resume,
	Session(u8),
	Advance(u16),
	SetDelay(u16),
	PttImmediate(u8, u8, u8),
}

fn step() -> impl Strategy<Value = Step> {
	let idx = 0u8..4;
	prop_oneof![
		2 => idx.clone().prop_map(Step::Attach),
		1 => idx.clone().prop_map(Step::Disconnect),
		3 => (idx.clone(), 0u8..3, any::<bool>()).prop_map(|(c, p, x)| Step::Start(c, p, x)),
		2 => (0u8..3).prop_map(Step::Invite),
		3 => (idx.clone(), idx.clone(), any::<bool>()).prop_map(|(c, k, t)| Step::Acquire(c, k, t)),
		1 => (idx.clone(), idx.clone()).prop_map(|(c, k)| Step::Release(c, k)),
		2 => (idx.clone(), idx.clone()).prop_map(|(c, k)| Step::Accept(c, k)),
		1 => (idx.clone(), idx.clone()).prop_map(|(c, k)| Step::Decline(c, k)),
		1 => (idx.clone(), idx.clone()).prop_map(|(c, k)| Step::End(c, k)),
		4 => (idx.clone(), idx.clone(), 0u8..4).prop_map(|(c, k, m)| Step::Route(c, k, m)),
		6 => (idx.clone(), idx.clone(), 0u8..3, any::<bool>(), any::<bool>())
			.prop_map(|(c, k, s, p, stale)| Step::Ptt(c, k, s, p, stale)),
		2 => (idx.clone(), idx.clone(), any::<bool>(), any::<bool>())
			.prop_map(|(c, k, m, d)| Step::CallControls(c, k, m, d)),
		2 => (any::<bool>(), any::<bool>()).prop_map(|(m, d)| Step::Global(m, d)),
		2 => (0u8..3).prop_map(Step::LinkUp),
		1 => (0u8..3).prop_map(Step::LinkDown),
		3 => idx.clone().prop_map(Step::RemoteAccept),
		1 => idx.clone().prop_map(Step::RemoteEnd),
		3 => idx.clone().prop_map(Step::MediaReady),
		1 => idx.clone().prop_map(Step::MediaStopped),
		3 => Just(Step::CaptureReady),
		1 => Just(Step::CaptureStale),
		1 => Just(Step::Unplug),
		1 => Just(Step::Resume),
		1 => (0u8..4).prop_map(Step::Session),
		4 => (0u16..400).prop_map(Step::Advance),
		2 => (0u16..2100).prop_map(Step::SetDelay),
		2 => (idx.clone(), idx.clone(), 0u8..3).prop_map(|(c, k, s)| Step::PttImmediate(c, k, s)),
	]
}

struct World {
	node: Node,
	clients: Vec<ClientId>,
	calls: Vec<CallId>,
	seq: BTreeMap<(u8, u8), u64>,
	watcher: FakeWatcher,
	route_gens: BTreeMap<CallId, RouteGeneration>,
	last_revision: Revision,
}

impl World {
	fn new() -> Self {
		let mut node = Node::new(0xA0);
		let clients = (0..4).map(|_| node.ids.client()).collect();
		let watcher = FakeWatcher::attach(&node.state);
		Self {
			node,
			clients,
			calls: Vec::new(),
			seq: BTreeMap::new(),
			watcher,
			route_gens: BTreeMap::new(),
			last_revision: Revision::ZERO,
		}
	}

	fn client(&self, c: u8) -> ClientId {
		self.clients[usize::from(c) % self.clients.len()]
	}

	fn call(&self, k: u8) -> Option<CallId> {
		(!self.calls.is_empty()).then(|| self.calls[usize::from(k) % self.calls.len()])
	}

	fn lease(&self, call: CallId, stale: bool) -> ControllerLeaseId {
		let current = self
			.node
			.state
			.call(call)
			.and_then(|c| c.controller)
			.map(|c| c.id);
		match (current, stale) {
			(Some(lease), false) => lease,
			_ => ControllerLeaseId::new(9_999),
		}
	}

	fn session(&self, call: CallId) -> SessionId {
		self.node
			.state
			.call(call)
			.map_or(SessionId::from_u128(0), |c| c.session)
	}

	fn run(&mut self, step: Step) {
		let peer = |p: u8| device_key(0xB0 + p);
		match step {
			Step::Attach(c) => drop(self.node.command(self.client(c), Command::Attach)),
			Step::Disconnect(c) => drop(self.node.feed(Input::Disconnected(self.client(c)))),
			Step::Start(c, p, parallel) => {
				let (call, session) = (self.node.ids.call(), self.node.ids.session());
				let cmd = Command::StartCall {
					call,
					session,
					peer: peer(p),
					allow_parallel_same_peer: parallel,
				};
				if self.node.command(self.client(c), cmd).is_ok() {
					self.calls.push(call);
				}
			}
			Step::Invite(p) => {
				let (call, session) = (self.node.ids.call(), self.node.ids.session());
				self.calls.push(call);
				self.node.feed(Input::Peer(PeerEvent::Invite {
					call,
					session,
					peer: peer(p),
					allow_parallel_same_peer: false,
				}));
			}
			Step::Acquire(c, k, takeover) => {
				if let Some(call) = self.call(k) {
					drop(self.node.command(
						self.client(c),
						Command::AcquireController { call, takeover },
					));
				}
			}
			Step::Release(c, k) => {
				if let Some(call) = self.call(k) {
					let lease = self.lease(call, false);
					drop(
						self.node
							.command(self.client(c), Command::ReleaseController { call, lease }),
					);
				}
			}
			Step::Accept(c, k) | Step::Decline(c, k) | Step::End(c, k) => {
				if let Some(call) = self.call(k) {
					let lease = self.lease(call, false);
					let cmd = match step {
						Step::Accept(..) => Command::AcceptCall { call, lease },
						Step::Decline(..) => Command::DeclineCall { call, lease },
						_ => Command::EndCall { call, lease },
					};
					drop(self.node.command(self.client(c), cmd));
				}
			}
			Step::Route(c, k, m) => {
				if let Some(call) = self.call(k) {
					let route = match m {
						0 => TransmitRoute::RECEIVE_ONLY,
						1 => TransmitRoute::open_mic(),
						2 => TransmitRoute::open_mic_background(),
						_ => TransmitRoute::push_to_talk(),
					};
					let lease = self.lease(call, false);
					drop(self.node.command(
						self.client(c),
						Command::SetTransmitRoute { call, lease, route },
					));
				}
			}
			Step::Ptt(c, k, s, pressed, stale) => {
				if let Some(call) = self.call(k) {
					let next = self.seq.entry((c, s)).or_insert(0);
					*next += 1;
					let sequence = if stale { next.saturating_sub(2) } else { *next };
					let update = PttUpdate {
						call,
						lease: self.lease(call, false),
						source: InputSourceId::new(u64::from(s)),
						sequence,
						pressed,
						ttl: None,
						immediate: false,
					};
					drop(self.node.command(self.client(c), Command::Ptt(update)));
				}
			}
			Step::CallControls(c, k, muted, deafened) => {
				if let Some(call) = self.call(k) {
					let lease = self.lease(call, false);
					let cmd = Command::SetCallControls {
						call,
						lease,
						muted: Some(muted),
						deafened: Some(deafened),
						gain: None,
					};
					drop(self.node.command(self.client(c), cmd));
				}
			}
			Step::Global(muted, deafened) => {
				let cmd = Command::SetGlobalControls {
					muted: Some(muted),
					deafened: Some(deafened),
					output_gain: None,
					ptt_release_delay: None,
					expected: None,
				};
				drop(self.node.command(self.client(0), cmd));
			}
			Step::LinkUp(p) => drop(self.node.link_up(peer(p))),
			Step::LinkDown(p) => drop(self.node.feed(Input::Peer(PeerEvent::LinkDown(peer(p))))),
			Step::RemoteAccept(k) => {
				self.remote(k, |call, session| PeerEvent::Accepted { call, session })
			}
			Step::RemoteEnd(k) => {
				self.remote(k, |call, session| PeerEvent::Ended { call, session })
			}
			Step::MediaReady(k) => {
				if let Some(call) = self.call(k) {
					let session = self.session(call);
					self.node
						.feed(Input::System(SystemEvent::MediaPrepared { call, session }));
					self.node
						.feed(Input::Peer(PeerEvent::PathValidated { call, session }));
				}
			}
			Step::MediaStopped(k) => {
				if let Some(call) = self.call(k) {
					let session = self.session(call);
					self.node
						.feed(Input::System(SystemEvent::MediaStopped { call, session }));
				}
			}
			Step::CaptureReady => self.node.complete_capture(),
			Step::CaptureStale => {
				let old = AudioGeneration::new(0);
				self.node
					.feed(Input::System(SystemEvent::CaptureReady { generation: old }));
			}
			Step::Unplug => {
				let generation = self.node.state.snapshot().global.capture_generation;
				self.node
					.feed(Input::System(SystemEvent::CaptureLost { generation }));
			}
			Step::Resume => drop(self.node.command(self.client(0), Command::ResumeCapture)),
			Step::Session(e) => {
				let event = [
					SessionEvent::Locked,
					SessionEvent::Unlocked,
					SessionEvent::Suspended,
					SessionEvent::Resumed,
				][usize::from(e)];
				self.node.feed(Input::System(SystemEvent::Session(event)));
			}
			Step::Advance(ms) => drop(self.node.advance(u64::from(ms))),
			Step::SetDelay(ms) => {
				let delay = std::time::Duration::from_millis(u64::from(ms));
				let command = Command::SetGlobalControls {
					muted: None,
					deafened: None,
					output_gain: None,
					ptt_release_delay: Some(delay),
					expected: None,
				};
				drop(self.node.command(self.client(0), command));
			}
			Step::PttImmediate(c, k, s) => {
				if let Some(call) = self.call(k) {
					let next = self.seq.entry((c, s)).or_insert(0);
					*next += 1;
					let sequence = *next;
					let update = PttUpdate {
						call,
						lease: self.lease(call, false),
						source: InputSourceId::new(u64::from(s)),
						sequence,
						pressed: false,
						ttl: None,
						immediate: true,
					};
					drop(self.node.command(self.client(c), Command::Ptt(update)));
				}
			}
		}
	}

	fn remote(&mut self, k: u8, event: impl Fn(CallId, SessionId) -> PeerEvent) {
		if let Some(call) = self.call(k) {
			let session = self.session(call);
			self.node.feed(Input::Peer(event(call, session)));
		}
	}

	fn check(&mut self) {
		let state = &self.node.state;
		let snap = state.snapshot();
		assert!(state.revision() >= self.last_revision);
		self.last_revision = state.revision();
		assert!(state.live_calls() <= 8);
		assert!(snap.clients.len() <= 32);

		let session_ok = !snap.global.locked && !snap.global.suspended;
		let mut needs_capture = false;
		for view in snap.calls.values() {
			let active = view.state == CallState::Active;
			if view.gate.open {
				assert!(active, "open gate on {:?}", view.state);
				assert!(view.route.is_authorized(), "no consent");
				assert!(!snap.global.muted && !snap.global.deafened);
				assert!(!view.controls.muted && !view.controls.deafened);
				assert_eq!(snap.global.capture, CaptureState::Ready);
				assert!(session_ok);
				assert!(snap.links.contains(&view.peer));
				assert!(view.controller.is_some() || view.route.background);
				if view.route.mode == RouteMode::PushToTalk {
					assert!(view.ptt_held);
				}
			}
			if view.releasing {
				// A pending hold only exists while every deny condition is clear.
				assert!(view.gate.open, "hold survived a deny");
			}
			if view.ptt_held {
				assert_eq!(view.route.mode, RouteMode::PushToTalk);
				assert!(view.controller.is_some());
				assert!(active);
			}
			if view.state.is_terminal() || view.state == CallState::Ending {
				assert_eq!(view.route, TransmitRoute::RECEIVE_ONLY);
				assert!(!view.gate.open);
			}
			if view.state.is_terminal() {
				assert!(view.controller.is_none());
			}
			if matches!(view.state, CallState::Connecting | CallState::Active)
				&& view.route.is_authorized()
			{
				needs_capture = true;
			}
			let previous = self.route_gens.insert(view.id, view.route_generation);
			assert!(previous.is_none_or(|p| p <= view.route_generation));
		}
		let capture_open = matches!(
			snap.global.capture,
			CaptureState::Opening | CaptureState::Ready
		);
		assert!(
			!capture_open || (needs_capture && session_ok),
			"capture open only under consent"
		);

		self.watcher.sync(&self.node.state);
		assert_eq!(self.watcher.view, snap, "watch fold diverged from snapshot");
		assert_eq!(self.watcher.resyncs, 0);
	}
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(256))]

	#[test]
	fn invariants_hold_for_any_interleaving(steps in proptest::collection::vec(step(), 1..160)) {
		let mut world = World::new();
		for step in steps {
			world.run(step);
			world.check();
			let effects = &world.node.effects;
			// Deny gates are published ahead of other effects within one outcome:
			// verified per outcome in the dedicated routing test; here, the
			// log must never contain an open gate for a non-active call.
			for effect in effects.iter().rev().take(8) {
				if let Effect::GateUpdate { gate, .. } = effect && gate.open {
					prop_assert!(gate.deny.is_none());
				}
			}
		}
	}

	#[test]
	fn gate_is_closed_if_any_single_deny_holds(
		muted in any::<bool>(), deafened in any::<bool>(), session in any::<bool>(),
		capture in any::<bool>(), active in any::<bool>(), link in any::<bool>(),
		mode in 0u8..3, authority in any::<bool>(), call_muted in any::<bool>(),
		call_deafened in any::<bool>(), ptt in any::<bool>(),
	) {
		let route = [RouteMode::ReceiveOnly, RouteMode::OpenMic, RouteMode::PushToTalk][usize::from(mode)];
		let inputs = GateInputs {
			global_muted: muted, global_deafened: deafened, session_allowed: session,
			capture_ready: capture, call_active: active, link_up: link, route,
			authority_valid: authority, call_muted, call_deafened, ptt_held: ptt,
		};
		let open = evaluate(&inputs).is_open();
		let any_deny = muted || deafened || !session || !capture || !active || !link
			|| route == RouteMode::ReceiveOnly || !authority || call_muted || call_deafened
			|| (route == RouteMode::PushToTalk && !ptt);
		prop_assert_eq!(open, !any_deny);
	}

	#[test]
	fn replaying_the_same_trace_is_deterministic(steps in proptest::collection::vec(step(), 1..80)) {
		let run = |steps: &[Step]| {
			let mut world = World::new();
			for step in steps {
				world.run(step.clone());
			}
			(world.node.state.snapshot(), world.node.effects)
		};
		prop_assert_eq!(run(&steps), run(&steps));
	}
}
