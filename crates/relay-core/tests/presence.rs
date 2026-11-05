//! Presence and ownership: with nobody attached an inbound invite is declined
//! as busy; a call belongs to the attached client that controls it and ends
//! (like a hangup) when that owner is gone for the grace period.

use std::time::Duration;

use proptest::prelude::*;
use relay_core::testing::{SeqIds, device_key};
use relay_core::{
	CallId, CallState, ClientId, Command, ControllerLeaseId, Effect, EndReason, Input, Limits,
	MonoTime, Outcome, PeerEvent, PeerMessage, Reject, Reply, Request, State, SystemEvent,
};

const GRACE: Duration = Duration::from_secs(3);

struct Rig {
	state: State,
	ids: SeqIds,
	now: u64,
}

impl Rig {
	fn tracked() -> Self {
		let mut ids = SeqIds::new();
		let state = State::new(device_key(1), ids.incarnation(), Limits::default()).unwrap();
		let mut rig = Self { state, ids, now: 0 };
		rig.system(SystemEvent::TrackPresence);
		rig
	}

	fn at(&self) -> MonoTime {
		MonoTime::from_millis(self.now)
	}

	fn system(&mut self, event: SystemEvent) -> Outcome {
		self.state.apply(Input::System(event), self.at())
	}

	fn advance(&mut self, by: Duration) -> Outcome {
		self.now += u64::try_from(by.as_millis()).unwrap();
		self.state.apply(Input::Tick, self.at())
	}

	fn client(&mut self, attached: bool) -> ClientId {
		let client = self.ids.client();
		let request = Request {
			client,
			operation: self.ids.operation(),
			command: Command::Attach,
		};
		self.state.apply(Input::Client(request), self.at());
		if attached {
			self.watch(client, true);
		}
		client
	}

	fn watch(&mut self, client: ClientId, watching: bool) {
		self.system(SystemEvent::Watching { client, watching });
	}

	fn command(&mut self, client: ClientId, command: Command) -> Outcome {
		let request = Request {
			client,
			operation: self.ids.operation(),
			command,
		};
		self.state.apply(Input::Client(request), self.at())
	}

	fn start_call(&mut self, client: ClientId, peer: u8) -> CallId {
		let call = self.ids.call();
		let session = self.ids.session();
		let outcome = self.command(
			client,
			Command::StartCall {
				call,
				session,
				peer: device_key(peer),
				allow_parallel_same_peer: false,
			},
		);
		assert!(
			matches!(outcome.result, Ok(Reply::Controller(_))),
			"{:?}",
			outcome.result
		);
		// With the link up the invitation goes out and the call rings.
		self.state
			.apply(Input::Peer(PeerEvent::LinkUp(device_key(peer))), self.at());
		call
	}

	fn invite(&mut self, peer: u8) -> (CallId, Outcome) {
		let call = self.ids.call();
		let outcome = self.state.apply(
			Input::Peer(PeerEvent::Invite {
				call,
				session: self.ids.session(),
				peer: device_key(peer),
				allow_parallel_same_peer: false,
			}),
			self.at(),
		);
		(call, outcome)
	}

	fn state_of(&self, call: CallId) -> CallState {
		self.state.snapshot().calls[&call].state
	}

	fn reason_of(&self, call: CallId) -> Option<EndReason> {
		self.state.snapshot().calls[&call].reason
	}

	/// An incoming call accepted by `client`, driven to Active.
	fn active_call(&mut self, client: ClientId, peer: u8) -> (CallId, ControllerLeaseId) {
		let (call, outcome) = self.invite(peer);
		assert!(outcome.result.is_ok());
		let lease = match self
			.command(
				client,
				Command::AcquireController {
					call,
					takeover: false,
				},
			)
			.result
		{
			Ok(Reply::Controller(lease)) => lease,
			other => panic!("{other:?}"),
		};
		self.command(client, Command::AcceptCall { call, lease });
		let session = self.state.call(call).unwrap().session;
		self.system(SystemEvent::MediaPrepared { call, session });
		self.state.apply(
			Input::Peer(PeerEvent::PathValidated { call, session }),
			self.at(),
		);
		assert_eq!(self.state_of(call), CallState::Active);
		(call, lease)
	}
}

fn opens_capture(effects: &[Effect]) -> bool {
	effects
		.iter()
		.any(|e| matches!(e, Effect::OpenCapture { .. }))
}

fn sends(effects: &[Effect], wanted: impl Fn(&PeerMessage) -> bool) -> bool {
	effects
		.iter()
		.any(|e| matches!(e, Effect::Peer { message, .. } if wanted(message)))
}

#[test]
fn an_invite_with_nobody_attached_is_declined_busy_and_never_rings() {
	let mut rig = Rig::tracked();
	rig.client(false); // registered, but not watching
	let (_, outcome) = rig.invite(2);
	assert_eq!(outcome.result, Err(Reject::NotAttached));
	assert!(sends(&outcome.effects, |m| matches!(
		m,
		PeerMessage::Decline { busy: true, .. }
	)));
	assert!(!opens_capture(&outcome.effects));
	assert!(rig.state.snapshot().calls.is_empty());
}

#[test]
fn a_watching_client_lets_invites_ring() {
	let mut rig = Rig::tracked();
	rig.client(true);
	let (call, outcome) = rig.invite(2);
	assert!(outcome.result.is_ok());
	assert_eq!(rig.state_of(call), CallState::IncomingRinging);
}

#[test]
fn untracked_engines_behave_as_before() {
	let mut ids = SeqIds::new();
	let mut state = State::new(device_key(1), ids.incarnation(), Limits::default()).unwrap();
	let outcome = state.apply(
		Input::Peer(PeerEvent::Invite {
			call: ids.call(),
			session: ids.session(),
			peer: device_key(2),
			allow_parallel_same_peer: false,
		}),
		MonoTime::ZERO,
	);
	assert!(outcome.result.is_ok());
}

#[test]
fn an_active_call_ends_when_its_owner_stays_away_past_the_grace() {
	let mut rig = Rig::tracked();
	let owner = rig.client(true);
	let (call, _) = rig.active_call(owner, 2);
	rig.watch(owner, false);

	// Within the grace nothing happens.
	rig.advance(GRACE - Duration::from_millis(1));
	assert_eq!(rig.state_of(call), CallState::Active);

	let outcome = rig.advance(Duration::from_millis(1));
	assert_eq!(rig.state_of(call), CallState::Ending);
	assert_eq!(rig.reason_of(call), Some(EndReason::OwnerGone));
	assert!(sends(&outcome.effects, |m| matches!(
		m,
		PeerMessage::End { .. }
	)));
	assert!(
		outcome
			.effects
			.iter()
			.any(|e| matches!(e, Effect::StopMedia { .. }))
	);
	let open_gate = outcome
		.effects
		.iter()
		.any(|e| matches!(e, Effect::GateUpdate { gate, .. } if gate.call == call && gate.open));
	assert!(!open_gate, "transmit stays closed");
}

#[test]
fn reattaching_within_the_grace_keeps_the_call() {
	let mut rig = Rig::tracked();
	let owner = rig.client(true);
	let (call, _) = rig.active_call(owner, 2);
	rig.watch(owner, false);
	rig.advance(GRACE / 2);
	rig.watch(owner, true);
	rig.advance(GRACE * 2);
	assert_eq!(rig.state_of(call), CallState::Active);
}

#[test]
fn a_takeover_by_an_attached_client_during_the_grace_transfers_ownership() {
	let mut rig = Rig::tracked();
	let owner = rig.client(true);
	let other = rig.client(true);
	let (call, _) = rig.active_call(owner, 2);
	rig.watch(owner, false);
	rig.advance(GRACE / 2);
	let taken = rig.command(
		other,
		Command::AcquireController {
			call,
			takeover: true,
		},
	);
	assert!(taken.result.is_ok());
	rig.advance(GRACE * 2);
	assert_eq!(rig.state_of(call), CallState::Active);
	// The new owner is now the one that counts.
	rig.watch(other, false);
	rig.advance(GRACE);
	assert_eq!(rig.state_of(call), CallState::Ending);
}

#[test]
fn only_the_departing_clients_calls_end() {
	let mut rig = Rig::tracked();
	let x = rig.client(true);
	let y = rig.client(true);
	let observer = rig.client(true);
	let first = rig.start_call(x, 2);
	let second = rig.start_call(y, 3);
	rig.watch(observer, false);
	rig.advance(GRACE * 2);
	assert_eq!(rig.state_of(first), CallState::OutgoingRinging);
	assert_eq!(rig.state_of(second), CallState::OutgoingRinging);

	rig.watch(x, false);
	rig.advance(GRACE);
	assert_eq!(rig.state_of(first), CallState::Cancelled);
	assert_eq!(rig.reason_of(first), Some(EndReason::OwnerGone));
	assert_eq!(
		rig.state_of(second),
		CallState::OutgoingRinging,
		"Y's call is untouched"
	);
}

#[test]
fn a_ringing_call_is_declined_when_the_last_client_leaves() {
	let mut rig = Rig::tracked();
	let watcher = rig.client(true);
	let (call, _) = rig.invite(2);
	rig.watch(watcher, false);
	let outcome = rig.advance(GRACE);
	assert_eq!(rig.state_of(call), CallState::Declined);
	assert!(sends(&outcome.effects, |m| matches!(
		m,
		PeerMessage::Decline { busy: true, .. }
	)));
}

#[test]
fn a_disconnected_owner_loses_the_call_too() {
	let mut rig = Rig::tracked();
	let owner = rig.client(true);
	let _watcher = rig.client(true);
	let (call, _) = rig.active_call(owner, 2);
	rig.state.apply(Input::Disconnected(owner), rig.at());
	rig.advance(GRACE);
	assert_eq!(
		rig.state_of(call),
		CallState::Ending,
		"other watchers do not own it"
	);
}

#[derive(Debug, Clone)]
enum Step {
	Watch(u8, bool),
	Call(u8),
	Invite,
	Release(u8),
	Wait(u16),
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
	proptest::collection::vec(
		prop_oneof![
			(0u8..3, any::<bool>()).prop_map(|(c, w)| Step::Watch(c, w)),
			(0u8..3).prop_map(Step::Call),
			Just(Step::Invite),
			(0u8..3).prop_map(Step::Release),
			(1u16..4000).prop_map(Step::Wait),
		],
		1..60,
	)
}

proptest! {
	/// No live call stays ownerless past the grace, and only an orphaned
	/// call is ever ended for it.
	#[test]
	fn no_call_outlives_its_owners_presence(plan in steps()) {
		let mut rig = Rig::tracked();
		let clients: Vec<ClientId> = (0..3).map(|_| rig.client(false)).collect();
		let mut watching = [false; 3];
		let mut since: std::collections::BTreeMap<CallId, u64> = Default::default();
		let mut peer = 10u8;
		for step in plan {
			match step {
				Step::Watch(c, w) => {
					watching[usize::from(c)] = w;
					rig.watch(clients[usize::from(c)], w);
				}
				Step::Call(c) => {
					peer = peer.wrapping_add(1);
					let call = rig.ids.call();
					let session = rig.ids.session();
					rig.command(clients[usize::from(c)], Command::StartCall {
						call, session, peer: device_key(peer), allow_parallel_same_peer: false,
					});
				}
				Step::Invite => {
					peer = peer.wrapping_add(1);
					rig.invite(peer);
				}
				Step::Release(c) => {
					let calls: Vec<CallId> = rig.state.snapshot().calls.keys().copied().collect();
					for call in calls {
						rig.command(clients[usize::from(c)], Command::ReleaseController {
							call,
							lease: ControllerLeaseId::new(0),
						});
					}
				}
				Step::Wait(ms) => {
					rig.advance(Duration::from_millis(u64::from(ms)));
				}
			}
			let snapshot = rig.state.snapshot();
			let any = watching.iter().any(|w| *w);
			for (id, view) in &snapshot.calls {
				let live = !view.state.is_terminal() && view.state != CallState::Ending;
				if !live {
					if view.reason == Some(EndReason::OwnerGone) {
						prop_assert!(since.contains_key(id), "ended as orphaned but never was");
					}
					continue;
				}
				let orphan = live && match (view.state, view.controller) {
					(CallState::IncomingRinging, None) => !any,
					(_, None) => true,
					(_, Some((holder, _))) => {
						let index = clients.iter().position(|c| *c == holder).unwrap();
						!watching[index]
					}
				};
				if orphan {
					let first = *since.entry(*id).or_insert(rig.now);
					prop_assert!(
						rig.now - first < GRACE.as_millis() as u64
							|| rig.now == first,
						"call {id} orphaned for {}ms", rig.now - first
					);
				} else {
					since.remove(id);
				}
			}
		}
	}
}
