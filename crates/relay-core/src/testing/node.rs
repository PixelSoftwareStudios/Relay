use crate::command::{Command, Input, PeerEvent, Request, SystemEvent};
use crate::effect::Effect;
use crate::id::{CallId, ClientId, ControllerLeaseId, PeerId, SessionId};
use crate::limits::Limits;
use crate::outcome::{OpResult, Outcome, Reply};
use crate::state::State;
use crate::testing::ids::device_key;
use crate::testing::{FakeClock, FakeDevice, FakeNetwork, SeqIds};
use crate::time::Clock;

/// A whole fake engine: state, clock, id source and fake adapters.
#[derive(Debug)]
pub struct Node {
	/// This node's device key.
	pub key: PeerId,
	/// The state owner under test.
	pub state: State,
	/// The shared fake clock.
	pub clock: FakeClock,
	/// Deterministic identifiers.
	pub ids: SeqIds,
	/// Fake capture device.
	pub device: FakeDevice,
	/// Fake peer network.
	pub network: FakeNetwork,
	/// Every effect ever produced, in order.
	pub effects: Vec<Effect>,
	media: Vec<(CallId, SessionId)>,
}

impl Node {
	/// A node whose device key bytes are all `key_byte`.
	pub fn new(key_byte: u8) -> Self {
		Self::with_limits(key_byte, Limits::default())
	}

	/// A node with custom limits.
	pub fn with_limits(key_byte: u8, limits: Limits) -> Self {
		let mut ids = SeqIds::namespaced(key_byte);
		let key = device_key(key_byte);
		let state = State::new(key, ids.incarnation(), limits).expect("valid limits");
		Self {
			key,
			state,
			clock: FakeClock::new(),
			ids,
			device: FakeDevice::default(),
			network: FakeNetwork::default(),
			effects: Vec::new(),
			media: Vec::new(),
		}
	}

	/// Applies an input at the fake clock's current time.
	pub fn feed(&mut self, input: Input) -> Outcome {
		let outcome = self.state.apply(input, self.clock.now());
		for effect in &outcome.effects {
			let handled = self.device.observe(effect) || self.network.observe(effect);
			if let Effect::PrepareMedia { call, session } = *effect {
				self.media.push((call, session));
			}
			debug_assert!(handled || !matches!(effect, Effect::Peer { .. }));
			self.effects.push(*effect);
		}
		outcome
	}

	/// Sends a client command with a fresh operation id.
	pub fn command(&mut self, client: ClientId, command: Command) -> OpResult {
		let operation = self.ids.operation();
		self.feed(Input::Client(Request {
			client,
			operation,
			command,
		}))
		.result
	}

	/// Attaches a new client and returns its id.
	pub fn attach_client(&mut self) -> ClientId {
		let client = self.ids.client();
		self.command(client, Command::Attach).expect("attach");
		client
	}

	/// Moves the clock and processes a tick.
	pub fn advance(&mut self, millis: u64) -> Outcome {
		self.clock.advance_millis(millis);
		self.feed(Input::Tick)
	}

	/// Brings the secure link to `peer` up.
	pub fn link_up(&mut self, peer: PeerId) -> Outcome {
		self.feed(Input::Peer(PeerEvent::LinkUp(peer)))
	}

	/// Starts a call as `client` and returns its id and controller lease.
	pub fn start_call(&mut self, client: ClientId, peer: PeerId) -> (CallId, ControllerLeaseId) {
		let call = self.ids.call();
		let session = self.ids.session();
		let result = self.command(
			client,
			Command::StartCall {
				call,
				session,
				peer,
				allow_parallel_same_peer: false,
			},
		);
		match result {
			Ok(Reply::Controller(lease)) => (call, lease),
			other => panic!("start_call failed: {other:?}"),
		}
	}

	/// Completes media preparation and path validation for queued requests,
	/// returning the number of calls handled.
	pub fn complete_media(&mut self) -> usize {
		let pending = std::mem::take(&mut self.media);
		for &(call, session) in &pending {
			self.feed(Input::System(SystemEvent::MediaPrepared { call, session }));
			self.feed(Input::Peer(PeerEvent::PathValidated { call, session }));
		}
		pending.len()
	}

	/// Completes the pending capture open, if any.
	pub fn complete_capture(&mut self) {
		let pending = self.device.pending().to_vec();
		for generation in pending {
			let input = self.device.ready(generation);
			self.feed(input);
		}
	}
}

/// Delivers messages between two nodes until both are quiet.
pub fn pump(a: &mut Node, b: &mut Node) {
	loop {
		let from_a = a.network.take_outbox();
		let from_b = b.network.take_outbox();
		let links_a = std::mem::take(&mut a.network.link_requests);
		let links_b = std::mem::take(&mut b.network.link_requests);
		let mut idle =
			from_a.is_empty() && from_b.is_empty() && links_a.is_empty() && links_b.is_empty();
		for _ in links_a.iter().chain(links_b.iter()) {
			a.link_up(b.key);
			b.link_up(a.key);
		}
		for (_, message) in from_a {
			b.feed(FakeNetwork::delivered(a.key, message));
		}
		for (_, message) in from_b {
			a.feed(FakeNetwork::delivered(b.key, message));
		}
		if a.complete_media() + b.complete_media() > 0 {
			idle = false;
		}
		if idle {
			return;
		}
	}
}
