//! The messaging actor: the single owner of every transcript, transfer record and event
//! revision. Everything reaches it as a closure on its inbox; time reaches it as a wakeup
//! computed from the stores' retry deadlines.

use std::collections::{HashMap, VecDeque};
use std::future::pending;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_api::pb::{self, SessionOpResult, session_event};
use relay_core::{CallId, PeerId};
use relay_messages::rate::TokenBucket;
use relay_messages::sanitize::sanitize_display_name;
use relay_messages::{
	HistoryManager, PeerFingerprint, PeerRef, Permissions, SessionId, SessionInfo, SessionStore,
	store::StoreAction,
};
use relay_net::identity::Fingerprint;
use relay_net::transfer::TransferHub;
use tokio::sync::{mpsc, watch};
use tokio::time::sleep_until;

use super::config::{ConfigStore, SharedConfig};
use super::feed::SessionFeed;
use super::handle::{INBOX, MessagingHandle, Work};
use super::session::Meta;
use super::sink::{MessagingLimits, SinkFactory};
use super::view;
use super::wire;
use crate::peers::PeerHandle;

/// Events replayed to a resuming watcher.
const REPLAY_EVENTS: usize = 2048;
/// Queue of a watcher before it is told to resync.
const WATCHER_QUEUE: usize = 256;
/// Remembered mutation outcomes.
const IDEMPOTENCY_ENTRIES: usize = 1024;

/// What the actor needs from the daemon.
pub struct HubParts {
	/// Peer links.
	pub peers: PeerHandle,
	/// Data-link listener and dialer; `None` when it could not be bound (no file transfers).
	pub transfers: Option<Arc<TransferHub>>,
	/// Local identity.
	pub local: Fingerprint,
	/// Live preferences.
	pub config: SharedConfig,
	/// Persists preference changes.
	pub store: Arc<dyn ConfigStore>,
	/// Destination sinks.
	pub sinks: SinkFactory,
	/// Transcript budgets.
	pub limits: MessagingLimits,
}

/// The phase of a call as far as messaging cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
	/// Accepted by both sides and not over.
	Live,
	/// Not started yet (ringing).
	Pending,
	/// Ending or over.
	Over,
}

/// One call as the control actor sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallFact {
	/// Call id.
	pub call: CallId,
	/// Remote peer.
	pub peer: PeerId,
	/// The local engine invited.
	pub outgoing: bool,
	/// Phase.
	pub phase: Phase,
}

pub(super) struct HubClock(Instant);

impl relay_messages::Clock for HubClock {
	fn wall_ms(&self) -> u64 {
		relay_io::clock::unix_millis()
	}
	fn mono_ms(&self) -> u64 {
		u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
	}
}

/// (client, operation).
type OpKey = ([u8; 16], [u8; 16]);
/// Request fingerprint and the outcome it produced.
type Stored = ([u8; 32], SessionOpResult);

/// Recent mutation outcomes by (client, operation).
#[derive(Default)]
pub(super) struct Idempotency {
	map: HashMap<OpKey, Stored>,
	order: VecDeque<OpKey>,
}

impl Idempotency {
	pub(super) fn get(&self, key: OpKey) -> Option<&Stored> {
		self.map.get(&key)
	}

	pub(super) fn put(&mut self, key: OpKey, print: [u8; 32], r: SessionOpResult) {
		if self.map.insert(key, (print, r)).is_none() {
			self.order.push_back(key);
			if self.order.len() > IDEMPOTENCY_ENTRIES
				&& let Some(old) = self.order.pop_front()
			{
				self.map.remove(&old);
			}
		}
	}
}

/// The messaging state.
pub struct Hub {
	pub(super) env: HubParts,
	pub(super) history: HistoryManager,
	pub(super) sessions: HashMap<SessionId, Meta>,
	pub(super) by_call: HashMap<CallId, SessionId>,
	pub(super) by_peer: HashMap<PeerId, SessionId>,
	pub(super) links: HashMap<PeerId, IpAddr>,
	pub(super) feed: SessionFeed,
	pub(super) idem: Idempotency,
	pub(super) stray: HashMap<PeerId, TokenBucket>,
	pub(super) rate: watch::Sender<Option<u64>>,
	pub(super) handle: MessagingHandle,
	pub(super) clock: Arc<HubClock>,
	origin: Instant,
	stopping: bool,
}

/// Starts the actor and returns its handle.
pub fn spawn(parts: HubParts) -> MessagingHandle {
	let (tx, rx) = mpsc::channel::<Work>(INBOX);
	let handle = MessagingHandle { tx };
	let hub = Hub::new(parts, handle.clone());
	tokio::spawn(hub.run(rx));
	handle
}

impl MessagingHandle {
	/// Cancels every transfer and stops the actor once its inbox drains.
	pub async fn stop(&self) {
		let _ = self.post(|hub| hub.stopping = true).await;
	}

	/// The control actor reports the calls it knows; sessions follow them.
	pub async fn calls(&self, facts: Vec<CallFact>) {
		let _ = self.post(move |hub| hub.calls_changed(&facts)).await;
	}

	/// Session traffic from the peer layer.
	pub async fn peer_event(&self, event: crate::peers::SessionEvent) {
		let _ = self.post(move |hub| hub.on_peer(event)).await;
	}
}

impl Hub {
	fn new(env: HubParts, handle: MessagingHandle) -> Self {
		let origin = Instant::now();
		let clock = Arc::new(HubClock(origin));
		let rate = watch::channel(env.config.load().rate_bytes_per_sec()).0;
		Self {
			history: HistoryManager::with_budget(clock.clone(), env.limits.retained_bytes),
			env,
			sessions: HashMap::new(),
			by_call: HashMap::new(),
			by_peer: HashMap::new(),
			links: HashMap::new(),
			feed: SessionFeed::new(REPLAY_EVENTS, WATCHER_QUEUE),
			idem: Idempotency::default(),
			stray: HashMap::new(),
			rate,
			handle,
			clock,
			origin,
			stopping: false,
		}
	}

	async fn run(mut self, mut inbox: mpsc::Receiver<Work>) {
		while !self.stopping {
			let wake = self.next_wakeup();
			tokio::select! {
				work = inbox.recv() => match work {
					Some(work) => work(&mut self),
					None => break,
				},
				() = wait_until(wake) => self.tick(),
			}
		}
		self.cancel_all();
	}

	fn next_wakeup(&self) -> Option<tokio::time::Instant> {
		self.by_call
			.values()
			.filter_map(|sid| self.history.get(*sid)?.next_deadline())
			.min()
			.map(|ms| (self.origin + Duration::from_millis(ms)).into())
	}

	/// Resends and give-ups that came due.
	fn tick(&mut self) {
		let ids: Vec<SessionId> = self.by_call.values().copied().collect();
		for sid in ids {
			let Some(store) = self.history.get_mut(sid) else {
				continue;
			};
			let actions = store.poll();
			let Some(peer) = self.sessions.get(&sid).map(|m| m.peer) else {
				continue;
			};
			for action in actions {
				match action {
					StoreAction::Resend(w) => {
						self.send(peer, wire::to_wire(&w));
					}
					StoreAction::Failed(seq) => self.emit_delivery(sid, seq),
				}
			}
		}
	}

	/// Puts a message on the peer's live trusted link; false when that is not possible.
	pub(super) fn send(&self, peer: PeerId, message: relay_net::proto::SessionMessage) -> bool {
		self.env.peers.send_session(peer, message).is_ok()
	}

	pub(super) fn cfg(&self) -> super::config::MessagingConfig {
		**self.env.config.load()
	}

	pub(super) fn store(&self, sid: SessionId) -> Option<&SessionStore> {
		self.history.get(sid)
	}

	pub(super) fn peer_name(&self, peer: PeerId) -> String {
		self.env
			.peers
			.peers()
			.iter()
			.find(|p| p.id == Some(peer))
			.map(|p| sanitize_display_name(&p.name))
			.unwrap_or_default()
	}

	// ----- call lifecycle -------------------------------------------------------------

	/// Starts sessions for calls that went live and ends those whose call is over.
	pub fn calls_changed(&mut self, facts: &[CallFact]) {
		for fact in facts {
			match fact.phase {
				Phase::Live
					if !self.by_call.contains_key(&fact.call)
						&& !self.by_peer.contains_key(&fact.peer) =>
				{
					self.start_session(*fact);
				}
				Phase::Over => self.end_call(fact.call),
				_ => {}
			}
		}
		let known: Vec<CallId> = self.by_call.keys().copied().collect();
		for call in known {
			if !facts.iter().any(|f| f.call == call) {
				self.end_call(call);
			}
		}
	}

	fn start_session(&mut self, fact: CallFact) {
		let id = SessionId(rand_id().to_be_bytes());
		let name = self.peer_name(fact.peer);
		let info = SessionInfo {
			id,
			peer: PeerRef {
				fingerprint: PeerFingerprint(*fact.peer.as_bytes()),
				display_name: name,
			},
			started_at_ms: relay_io::clock::unix_millis(),
		};
		let store = SessionStore::with_limit(
			info,
			Permissions::default(),
			self.clock.clone(),
			self.env.limits.session_bytes,
		);
		let replaced = self.history.start_with(store);
		for old in replaced {
			self.forget(old, "replaced");
		}
		self.sessions.insert(
			id,
			Meta {
				id,
				call: fact.call,
				peer: fact.peer,
				host: fact.outgoing,
				base: 0,
				xfers: HashMap::new(),
			},
		);
		self.by_call.insert(fact.call, id);
		self.by_peer.insert(fact.peer, id);
		tracing::debug!(session = %id.to_hex(), "messaging session started");
		self.emit_summary(id);
	}

	fn end_call(&mut self, call: CallId) {
		let Some(sid) = self.by_call.remove(&call) else {
			return;
		};
		if let Some(meta) = self.sessions.get(&sid) {
			self.by_peer.remove(&meta.peer);
		}
		self.cancel_session_transfers(sid, true);
		let evicted = self.history.end_session(sid);
		tracing::debug!(session = %sid.to_hex(), "messaging session ended");
		self.emit_summary(sid);
		for old in evicted {
			self.forget(old, "retained history budget");
		}
	}

	/// Drops the bookkeeping of a transcript the history manager removed.
	pub(super) fn forget(&mut self, sid: SessionId, reason: &str) {
		self.sessions.remove(&sid);
		self.emit(
			sid,
			session_event::Event::Evicted(pb::HistoryEvicted {
				session_id: sid.0.to_vec(),
				reason: reason.to_owned(),
			}),
		);
	}

	fn cancel_all(&mut self) {
		let ids: Vec<SessionId> = self.sessions.keys().copied().collect();
		for sid in ids {
			self.cancel_session_transfers(sid, true);
		}
	}

	// ----- events ---------------------------------------------------------------------

	pub(super) fn emit(&mut self, sid: SessionId, event: session_event::Event) {
		self.feed
			.push(sid.0, pb::SessionEvent { event: Some(event) });
	}

	pub(super) fn summary(&self, sid: SessionId) -> Option<pb::SessionSummary> {
		let (meta, store) = (self.sessions.get(&sid)?, self.history.get(sid)?);
		Some(view::summary(meta, store))
	}

	pub(super) fn emit_summary(&mut self, sid: SessionId) {
		if let Some(summary) = self.summary(sid) {
			self.emit(sid, session_event::Event::Session(summary));
		}
	}

	/// A new transcript entry (the last one stored).
	pub(super) fn emit_last_entry(&mut self, sid: SessionId) {
		let Some((meta, store)) = self.sessions.get(&sid).zip(self.history.get(sid)) else {
			return;
		};
		let index = store.messages().len().saturating_sub(1);
		let Some(message) = store.messages().get(index) else {
			return;
		};
		let entry = view::entry(view::ordinal(meta, index), message, meta);
		self.emit(
			sid,
			session_event::Event::Entry(pb::NewEntry {
				session_id: sid.0.to_vec(),
				entry: Some(entry),
			}),
		);
	}

	pub(super) fn emit_delivery(&mut self, sid: SessionId, seq: u64) {
		let Some(store) = self.history.get(sid) else {
			return;
		};
		let id = relay_messages::MessageId {
			origin: relay_messages::Origin::Local,
			seq,
		};
		let Some(message) = store.get(id) else {
			return;
		};
		let (state, failure) = view::delivery(message.delivery);
		self.emit(
			sid,
			session_event::Event::Delivery(pb::DeliveryChanged {
				session_id: sid.0.to_vec(),
				seq,
				delivery: state as i32,
				failure: failure.to_owned(),
			}),
		);
	}

	/// The full offer record after any change to it.
	pub(super) fn emit_offer(&mut self, sid: SessionId, offer: relay_messages::OfferId) {
		let Some((meta, store)) = self.sessions.get(&sid).zip(self.history.get(sid)) else {
			return;
		};
		let Some((index, message)) = store.messages().iter().enumerate().find(
			|(_, m)| matches!(&m.body, relay_messages::MessageBody::File(o) if o.id == offer),
		) else {
			return;
		};
		let relay_messages::MessageBody::File(file) = &message.body else {
			return;
		};
		let body = view::offer_body(
			file,
			message.id.origin == relay_messages::Origin::Local,
			meta.xfers.get(&offer),
		);
		let ordinal = view::ordinal(meta, index);
		self.emit(
			sid,
			session_event::Event::Offer(pb::OfferChanged {
				session_id: sid.0.to_vec(),
				ordinal,
				offer: Some(body),
			}),
		);
	}
}

fn rand_id() -> u128 {
	relay_io::entropy::OsEntropy.next_u128()
}

async fn wait_until(at: Option<tokio::time::Instant>) {
	match at {
		Some(at) => sleep_until(at).await,
		None => pending().await,
	}
}
