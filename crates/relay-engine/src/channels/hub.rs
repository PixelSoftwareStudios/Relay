//! The driver handle: shared state plus the pieces every part needs.

use relay_io::LockExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use relay_api::pb::{ChannelInfo, Operation};
use relay_core::channel::{ChannelId, Memberships};
use relay_core::{ClientId, OperationId};
use relay_net::channel::{AdmissionService, AttemptLimiter, ChannelStore};
use relay_net::identity::Fingerprint;
use tokio::sync::watch;

use super::clock::Clock;
use super::failure::Failure;
use super::ledger::{Ledger, Replay, Stored};
use super::state::State;
use super::view::{self, ChannelView};
use crate::peers::{AdvertSpec, PeerHandle};
use crate::service::{ControlHandle, SettingsStore};

/// Everything the driver is built from.
pub struct HubParts {
	/// Persistent channel records (secrets live in the store's vault).
	pub store: Arc<dyn ChannelStore>,
	/// The local device fingerprint.
	pub local: Fingerprint,
	/// The peer manager, for sending channel messages and dialling hinted peers.
	pub peers: PeerHandle,
	/// The control actor, told about channel changes.
	pub control: ControlHandle,
	/// Persisted privacy choices (inbound policy, advert mode).
	pub settings: Arc<dyn SettingsStore>,
	/// Time source.
	pub clock: Clock,
	/// Where the advertisement's channel part is published.
	pub advert: Arc<watch::Sender<AdvertSpec>>,
}

pub(super) struct Inner {
	pub store: Arc<dyn ChannelStore>,
	pub admission: AdmissionService,
	/// Pacing of this device's own join attempts (lockout after repeated failures).
	pub outbound: Mutex<AttemptLimiter>,
	pub local: Fingerprint,
	pub peers: PeerHandle,
	pub control: ControlHandle,
	pub settings: Arc<dyn SettingsStore>,
	pub clock: Clock,
	pub advert: Arc<watch::Sender<AdvertSpec>>,
	pub state: Mutex<State>,
	pub ledger: Mutex<Ledger>,
	pub revision: AtomicU64,
	/// Seeds the rotating pseudonymous instance id of hidden mode.
	pub instance_secret: [u8; 32],
	/// The stable instance id of visible mode.
	pub open_instance: [u8; 16],
}

/// The channel driver. Cheap to clone.
#[derive(Clone)]
pub struct ChannelHub {
	pub(super) inner: Arc<Inner>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
	mutex.lock_unpoisoned()
}

impl ChannelHub {
	/// Loads the stored channels and publishes the first advertisement.
	pub fn new(parts: HubParts) -> Result<Self, Failure> {
		let HubParts {
			store,
			local,
			peers,
			control,
			settings,
			clock,
			advert,
		} = parts;
		let hub = Self {
			inner: Arc::new(Inner {
				admission: AdmissionService::new(Arc::clone(&store)),
				store,
				outbound: Mutex::new(AttemptLimiter::new()),
				local,
				peers,
				control,
				settings,
				clock,
				advert,
				state: Mutex::new(State::default()),
				ledger: Mutex::new(Ledger::new()),
				revision: AtomicU64::new(0),
				instance_secret: rand::random(),
				open_instance: rand::random(),
			}),
		};
		hub.reload()?;
		hub.refresh_advert();
		Ok(hub)
	}

	pub(super) fn state(&self) -> MutexGuard<'_, State> {
		lock(&self.inner.state)
	}

	pub(super) fn now(&self) -> u64 {
		self.inner.clock.unix()
	}

	pub(super) fn mono(&self) -> Duration {
		self.inner.clock.mono()
	}

	/// Re-reads the store and rebuilds the joined set. Scope entries of channels
	/// that are gone are dropped.
	pub(super) fn reload(&self) -> Result<(), Failure> {
		let records = self.inner.store.list().map_err(|error| {
			tracing::warn!(%error, "channel store could not be read");
			Failure::Storage
		})?;
		let mut memberships = Memberships::new();
		for record in &records {
			let _ = memberships.insert(record.id, record.entry());
		}
		let mut state = self.state();
		let gone: Vec<ChannelId> = state
			.memberships
			.iter()
			.map(|(id, _)| *id)
			.filter(|id| !memberships.contains(id))
			.collect();
		for scope in state.scopes.values_mut() {
			gone.iter().for_each(|id| scope.forget(id));
		}
		state.proofs.retain(|(_, id), _| !gone.contains(id));
		state.records = records;
		state.memberships = memberships;
		Ok(())
	}

	/// Tells the control service what changed. Failures only mean it stopped.
	pub(super) async fn publish(&self) {
		let view = self.view();
		if let Ok(revision) = self.inner.control.channels_changed(view).await {
			self.inner.revision.store(revision, Ordering::Release);
		}
	}

	/// Channel information and per-peer proven sets.
	pub fn view(&self) -> ChannelView {
		let state = self.state();
		view::build(
			&state.records,
			state.scopes.iter(),
			state.nearby(self.mono()),
		)
	}

	/// The channels as `ListChannels` reports them.
	pub fn list(&self) -> Vec<ChannelInfo> {
		self.view().channels
	}

	/// The API revision seen at the last publish; used for operation results.
	pub fn revision(&self) -> u64 {
		self.inner.revision.load(Ordering::Acquire)
	}

	/// Channels the peer proved on its live link.
	pub fn proven_with(&self, peer: relay_core::PeerId) -> Vec<ChannelId> {
		self.state().proven(&peer)
	}

	/// Looks an operation up; returns the request print to record with.
	pub fn replay(
		&self,
		client: ClientId,
		operation: OperationId,
		encoded_request: &[u8],
	) -> (Replay, [u8; 32]) {
		let mut ledger = lock(&self.inner.ledger);
		let print = ledger.print(encoded_request);
		let found = ledger.lookup(client, operation, &print, self.mono());
		(found, print)
	}

	/// Stores an outcome for replay and polling.
	pub fn remember(
		&self,
		client: ClientId,
		operation: OperationId,
		print: [u8; 32],
		stored: Stored,
	) {
		lock(&self.inner.ledger).record(client, operation, print, stored, self.mono());
	}

	/// A recorded channel operation, for `GetOperation`.
	pub fn operation(&self, client: ClientId, operation: OperationId) -> Option<Operation> {
		lock(&self.inner.ledger).get(client, operation)
	}
}
