//! Discovery side: what this device advertises per channel and privacy mode, how a
//! hinted advertisement is followed up, and the periodic upkeep.

use std::time::Duration;

use relay_core::channel::ChannelId;
use relay_net::channel::limits::CERT_RENEW_GRACE;
use relay_net::channel::tag::{self, ChannelAdvert};
use relay_net::discovery::DiscoveredPeer;
use relay_net::identity::Fingerprint;

use super::hub::ChannelHub;
use super::state::{AdvertKey, DIAL_MEMORY_MAX, DIAL_WINDOW, DIALS_PER_WINDOW};
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
				&self.inner.local,
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

	/// A discovery record arrived. A recognised blinded tag names the trusted device
	/// that produced it: dial exactly that fingerprint so a proof can follow. A copied
	/// advertisement can only lead to a pinned dial that fails at the handshake.
	/// Nothing here counts as shared.
	///
	/// True when the nearby listed channels changed, so the caller publishes.
	pub(super) fn on_advert(&self, found: &DiscoveredPeer) -> bool {
		let Some(advert) = ChannelAdvert::from_map(&found.txt) else {
			return false;
		};
		let now = self.now();
		let mono = self.mono();
		let changed = self.note_listed(&advert, found.instance_id);
		if found.instance_id == self.advert().instance_id {
			return changed;
		}
		let known: Vec<Fingerprint> = self
			.inner
			.peers
			.trusted()
			.into_iter()
			.filter(|fp| *fp != self.inner.local)
			.collect();
		let mut state = self.state();
		let local: Vec<ChannelId> = state.memberships.iter().map(|(id, _)| *id).collect();
		let hits = advert.recognise(&local, &found.instance_id, &known, tag::epoch_of(now));
		let mut targets: Vec<(Fingerprint, Vec<ChannelId>)> = Vec::new();
		for hit in hits {
			match targets.iter_mut().find(|(fp, _)| *fp == hit.fingerprint) {
				Some((_, channels)) => channels.push(hit.channel),
				None => targets.push((hit.fingerprint, vec![hit.channel])),
			}
		}
		targets.retain(|(fp, _)| !state.links.contains_key(&peer_of(fp)));
		targets.retain(|(fp, _)| {
			state
				.dialled
				.get(fp)
				.is_none_or(|at| mono.saturating_sub(*at) >= REDIAL_AFTER)
		});
		if state.dialled.len() >= DIAL_MEMORY_MAX {
			state
				.dialled
				.retain(|_, at| mono.saturating_sub(*at) < REDIAL_AFTER);
		}
		// Global pace: a flood of recognised copies cannot cause a dial storm.
		state
			.dial_log
			.retain(|at| mono.saturating_sub(*at) < DIAL_WINDOW);
		let room = DIALS_PER_WINDOW.saturating_sub(state.dial_log.len());
		let room = room.min(DIAL_MEMORY_MAX.saturating_sub(state.dialled.len()));
		targets.truncate(room);
		for (fp, _) in &targets {
			state.dialled.insert(*fp, mono);
			state.dial_log.push_back(mono);
		}
		drop(state);
		let addresses: Vec<_> = found
			.addresses
			.iter()
			.copied()
			.take(ADDRESSES_PER_ADVERT)
			.collect();
		for (fingerprint, channels) in targets {
			let (hub, addresses) = (self.clone(), addresses.clone());
			tokio::spawn(async move {
				let peer = peer_of(&fingerprint);
				for address in addresses {
					if hub.inner.peers.dial(address, Some(peer)).await.is_err() {
						continue;
					}
					let mut state = hub.state();
					for id in channels {
						state.note(peer, id, relay_core::channel::Evidence::Candidate);
					}
					break;
				}
			});
		}
		changed
	}

	/// Remembers the listed channels of one advertisement (see `nearby.rs` for the rules).
	fn note_listed(&self, advert: &ChannelAdvert, device: [u8; 16]) -> bool {
		let mono = self.mono();
		let mut state = self.state();
		let mut changed = false;
		for listed in &advert.listed {
			changed |= state.note_listed(listed.name.as_str(), listed.id, device, mono);
		}
		changed
	}

	/// Periodic upkeep: follow the privacy setting, rotate the epoch, keep the
	/// certificates of authority holders fresh and ask for renewals. Public so tests
	/// can drive it after moving the clock.
	pub async fn tick(&self) {
		self.drop_stale_admissions();
		self.keep_own_certificates();
		let mono = self.mono();
		let nearby_moved = self.state().prune_listed(mono);
		if self.expire_proofs() || nearby_moved {
			self.publish().await;
		}
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

fn peer_of(fingerprint: &Fingerprint) -> relay_core::PeerId {
	relay_core::PeerId::from_bytes(*fingerprint.as_bytes())
}
