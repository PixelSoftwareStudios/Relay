//! The driver's mutable state. One mutex, never held across an await.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::time::Duration;

use relay_core::PeerId;
use relay_core::channel::{ChannelId, Memberships, PeerScope};
use relay_net::channel::{AdmitSession, ChannelRecord, JoinClient, SessionBinding};
use relay_net::identity::Fingerprint;
use tokio::sync::oneshot;

use super::failure::Failure;

/// Listed channel names remembered from adverts (for joining by name).
pub(super) const LISTED_SEEN_MAX: usize = 32;
/// A proof already sent for a channel is not repeated on a link sooner than this.
pub(super) const REPROOF_AFTER: Duration = Duration::from_secs(60);
/// How long before a proven certificate lapses the peer is asked again.
pub(super) const REPROOF_LEAD: u64 = 120;
/// Channel messages one link may send per second before the rest are dropped.
pub(super) const MESSAGES_PER_SEC: u32 = 20;
/// Dials started from recognised adverts: at most this many per window, over all devices.
pub(super) const DIALS_PER_WINDOW: usize = 4;
pub(super) const DIAL_WINDOW: Duration = Duration::from_secs(10);
/// Dial attempts remembered per advertising device.
pub(super) const DIAL_MEMORY_MAX: usize = 256;

pub(super) type JoinDone = oneshot::Sender<Result<(), Failure>>;

/// A join this device started on one link, waiting for the admitter.
pub(super) struct JoinSlot {
	pub client: JoinClient,
	pub done: JoinDone,
}

/// What the driver tracks per trusted link.
pub(super) struct Link {
	pub binding: SessionBinding,
	/// Channels we sent our certificate for on this link, and when (mono).
	pub replied: BTreeMap<ChannelId, Duration>,
	/// Channels we sent our reference for on this link.
	pub sent_ref: BTreeSet<ChannelId>,
	/// Channels the peer already sent a matching reference for on this link.
	pub got_ref: BTreeSet<ChannelId>,
	/// Inbound message budget of this link.
	pub budget: Budget,
	/// The single admission exchange this link may run as admitter, and when it began.
	/// A joiner that sees a bad confirmation just stops, so stale ones are dropped.
	pub admit: Option<(AdmitSession, Duration)>,
	/// The single join this link may run as joiner.
	pub join: Option<JoinSlot>,
	/// Renewals requested on this link and not yet answered.
	pub renew: BTreeSet<ChannelId>,
}

impl Link {
	pub fn new(binding: SessionBinding) -> Self {
		Self {
			binding,
			replied: BTreeMap::new(),
			sent_ref: BTreeSet::new(),
			got_ref: BTreeSet::new(),
			budget: Budget::default(),
			admit: None,
			join: None,
			renew: BTreeSet::new(),
		}
	}
}

/// Fixed one-second window counter for inbound channel messages.
#[derive(Default)]
pub(super) struct Budget {
	window: u64,
	used: u32,
}

impl Budget {
	/// Spends one message at `mono`; false once the window's allowance is gone.
	pub fn allow(&mut self, mono: Duration) -> bool {
		let window = mono.as_secs();
		if window != self.window {
			self.window = window;
			self.used = 0;
		}
		self.used = self.used.saturating_add(1);
		self.used <= MESSAGES_PER_SEC
	}
}

/// Expiry of a peer's proven certificate.
pub(super) struct ProofEntry {
	pub expires_at: u64,
	/// The peer was already asked to prove again before expiry.
	pub asked: bool,
}

/// What the last published advertisement was built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AdvertKey {
	pub epoch: u64,
	pub hidden: bool,
	pub channels: Vec<ChannelId>,
}

#[derive(Default)]
pub(super) struct State {
	/// Local copy of the stored records. The store stays authoritative for
	/// admission (`update` is atomic there); this serves proof checks.
	pub records: Vec<ChannelRecord>,
	pub memberships: Memberships,
	pub links: BTreeMap<PeerId, Link>,
	pub scopes: BTreeMap<PeerId, PeerScope>,
	/// Expiry of every proven certificate, so the upkeep tick can demote lapsed proofs.
	pub proofs: BTreeMap<(PeerId, ChannelId), ProofEntry>,
	/// Listed name to the first id seen; `None` when two ids claimed the name.
	pub listed_seen: BTreeMap<String, Option<ChannelId>>,
	pub dialled: HashMap<Fingerprint, Duration>,
	pub dial_log: VecDeque<Duration>,
	pub advert_key: Option<AdvertKey>,
}

impl State {
	/// Channels `peer` proved on its live link.
	pub fn proven(&self, peer: &PeerId) -> Vec<ChannelId> {
		self.scopes
			.get(peer)
			.map(|scope| scope.proven().collect())
			.unwrap_or_default()
	}

	/// Forgets everything proven by `peer` (its link changed).
	pub fn forget_peer(&mut self, peer: &PeerId) {
		self.scopes.remove(peer);
		self.proofs.retain(|(p, _), _| p != peer);
	}

	pub fn note(&mut self, peer: PeerId, id: ChannelId, evidence: relay_core::channel::Evidence) {
		let scope = self.scopes.entry(peer).or_default();
		scope.note(&self.memberships, id, evidence);
	}
}
