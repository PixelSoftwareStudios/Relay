//! Discovery side: what this device advertises per channel and privacy mode, how a
//! hinted advertisement is followed up, and the periodic upkeep.

use std::time::Duration;

use relay_core::channel::ChannelId;
use relay_net::channel::limits::CERT_RENEW_GRACE;
use relay_net::channel::tag::{self, ChannelAdvert};
use relay_net::discovery::DiscoveredPeer;

use super::hub::ChannelHub;
use super::state::{AdvertKey, DIAL_MEMORY_MAX, LISTED_SEEN_MAX};
use crate::peers::AdvertSpec;
use crate::service::ports::AdvertMode;

/// A hinted device is not redialled sooner than this.
const REDIAL_AFTER: Duration = Duration::from_secs(30);
/// Longest TXT value discovery accepts.
const TXT_VALUE_MAX: usize = 200;
/// Maintenance period.
const TICK: Duration = Duration::from_secs(1);
/// An admission exchange not completed in this time is abandoned.
const ADMISSION_PATIENCE: Duration = Duration::from_secs(15);
/// Dial at most this many addresses of one advertisement.
const ADDRESSES_PER_ADVERT: usize = 2;

impl ChannelHub {
	fn hidden(&self) -> bool {
		self.inner.settings.load().advert_mode == AdvertMode::Hidden
	}

	/// Rebuilds the advertisement if its inputs changed: the joined channels, the
	/// privacy mode or the tag epoch. Tags carry random decoys, so an unchanged input
	/// must not be rebuilt (it would re-announce for nothing).
	pub(super) fn refresh_advert(&self) {
		let hidden = self.hidden();
		let epoch = tag::epoch_of(self.now());
		let mut state = self.state();
		let channels: Vec<ChannelId> = state.memberships.iter().map(|(id, _)| *id).collect();
		let key = AdvertKey {
			epoch,
			hidden,
			channels,
		};
		if state.advert_key.as_ref() == Some(&key) {
			return;
		}
		let instance_id = if hidden {
			tag::hidden_instance_id(&self.inner.instance_secret, epoch)
		} else {
			self.inner.open_instance
		};
		// A device in no channel says nothing about channels, not even an epoch.
		let extra_txt = if key.channels.is_empty() {
			Vec::new()
		} else {
			let built = ChannelAdvert::build(
				state
					.memberships
					.iter()
					.map(|(id, entry)| (id, &entry.name, &entry.policy)),
				epoch,
				&instance_id,
			);
			built
				.to_txt()
				.into_iter()
				.filter(|(_, value)| value.is_ascii() && value.len() <= TXT_VALUE_MAX)
				.collect()
		};
		state.advert_key = Some(key);
		drop(state);
		// The privacy flag belongs to the peer adapter; only the channel part is ours.
		self.inner.advert.send_modify(|spec| {
			spec.instance_id = instance_id;
			spec.extra_txt = extra_txt;
		});
	}

	/// The advertisement currently published (tests build discovery records from it).
	pub fn advert(&self) -> AdvertSpec {
		self.inner.advert.borrow().clone()
	}

	/// A discovery record arrived. A recognised blinded tag is a hint that a member
	/// may live there: dial it so a proof can follow once trust allows. Nothing here
	/// counts as shared.
	pub(super) fn on_advert(&self, found: &DiscoveredPeer) {
		let Some(advert) = ChannelAdvert::from_map(&found.txt) else {
			return;
		};
		let now = self.now();
		let mono = self.mono();
		let mut state = self.state();
		for listed in &advert.listed {
			if !state.memberships.contains(&listed.id)
				&& (state.listed_seen.len() < LISTED_SEEN_MAX
					|| state.listed_seen.contains_key(listed.name.as_str()))
			{
				state
					.listed_seen
					.insert(listed.name.as_str().to_owned(), listed.id);
			}
		}
		if found.instance_id == self.advert().instance_id {
			return;
		}
		let local: Vec<ChannelId> = state.memberships.iter().map(|(id, _)| *id).collect();
		let hits = advert.recognise(&local, &found.instance_id, tag::epoch_of(now));
		if hits.is_empty() {
			return;
		}
		if state
			.dialled
			.get(&found.instance_id)
			.is_some_and(|at| mono.saturating_sub(*at) < REDIAL_AFTER)
		{
			return;
		}
		if state.dialled.len() >= DIAL_MEMORY_MAX {
			state
				.dialled
				.retain(|_, at| mono.saturating_sub(*at) < REDIAL_AFTER);
		}
		if state.dialled.len() >= DIAL_MEMORY_MAX {
			return;
		}
		state.dialled.insert(found.instance_id, mono);
		drop(state);
		let addresses: Vec<_> = found
			.addresses
			.iter()
			.copied()
			.take(ADDRESSES_PER_ADVERT)
			.collect();
		let hub = self.clone();
		tokio::spawn(async move {
			for address in addresses {
				let Ok(peer) = hub.inner.peers.dial(address, None).await else {
					continue;
				};
				if peer != relay_core::PeerId::from_bytes(*hub.inner.local.as_bytes()) {
					let mut state = hub.state();
					for id in hits {
						state.note(peer, id, relay_core::channel::Evidence::Candidate);
					}
				}
				break;
			}
		});
	}

	/// Periodic upkeep: follow the privacy setting, rotate the epoch, keep the
	/// certificates of authority holders fresh and ask for renewals. Public so tests
	/// can drive it after moving the clock.
	pub async fn tick(&self) {
		self.drop_stale_admissions();
		self.keep_own_certificates();
		self.refresh_advert();
		let renewals = self.due_renewals();
		for (peer, channel) in renewals {
			self.request_renewal(peer, channel);
		}
	}

	/// Starts the upkeep loop.
	pub fn spawn_maintenance(&self) -> tokio::task::JoinHandle<()> {
		let hub = self.clone();
		tokio::spawn(async move {
			loop {
				tokio::time::sleep(TICK).await;
				hub.tick().await;
			}
		})
	}

	/// An admission the joiner abandoned holds one of the few concurrent slots.
	fn drop_stale_admissions(&self) {
		let mono = self.mono();
		for link in self.state().links.values_mut() {
			if link
				.admit
				.as_ref()
				.is_some_and(|(_, began)| mono.saturating_sub(*began) > ADMISSION_PATIENCE)
			{
				link.admit = None;
			}
		}
	}

	fn keep_own_certificates(&self) {
		let (now, local) = (self.now(), self.inner.local);
		let stale: Vec<ChannelId> = self
			.state()
			.records
			.iter()
			.filter(|r| r.authority.is_some())
			.filter(|r| {
				r.cert
					.as_ref()
					.is_none_or(|c| c.expires_at.saturating_sub(now) < 86_400)
			})
			.map(|r| r.id)
			.collect();
		if stale.is_empty() {
			return;
		}
		for id in stale {
			let _ = self
				.inner
				.store
				.update(&id, &mut |record| record.refresh_own_cert(&local, now));
		}
		let _ = self.reload();
	}

	/// How close to expiry a member certificate gets before renewal is requested.
	pub(super) fn renew_window() -> u64 {
		CERT_RENEW_GRACE.as_secs() / 2
	}
}
