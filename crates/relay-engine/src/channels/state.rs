//! The driver's mutable state. One mutex, never held across an await.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use relay_core::PeerId;
use relay_core::channel::{ChannelId, Memberships, PeerScope};
use relay_net::channel::{AdmitSession, ChannelRecord, JoinClient, SessionBinding};
use tokio::sync::oneshot;

use super::failure::Failure;

/// Listed channel names remembered from adverts (for joining by name).
pub(super) const LISTED_SEEN_MAX: usize = 32;
/// Dial attempts remembered per advert instance.
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
	/// Channels we already answered a proof for on this link (answer once).
	pub replied: BTreeSet<ChannelId>,
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
			replied: BTreeSet::new(),
			admit: None,
			join: None,
			renew: BTreeSet::new(),
		}
	}
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
	pub listed_seen: BTreeMap<String, ChannelId>,
	pub dialled: HashMap<[u8; 16], Duration>,
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

	pub fn note(&mut self, peer: PeerId, id: ChannelId, evidence: relay_core::channel::Evidence) {
		let scope = self.scopes.entry(peer).or_default();
		scope.note(&self.memberships, id, evidence);
	}
}
