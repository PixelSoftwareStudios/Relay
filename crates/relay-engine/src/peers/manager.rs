//! The peer manager actor: owns every control link, the trust gate, link
//! election, reconnect timers and the mapping to domain events.

use relay_io::LockExt;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use relay_core::PeerEvent;
use relay_net::channel::SessionBinding;
use relay_net::discovery::DiscoveryTable;
use relay_net::handshake::UntrustedLinks;
use relay_net::identity::{Fingerprint, PeerTrust, TrustRecord, TrustState, evaluate_peer};
use relay_net::proto::{Envelope, envelope::Body};
use tokio::sync::{mpsc, oneshot};

use super::dialing::Want;
use super::election::{LinkFacts, Verdict, elect};
use super::handle::{
	ChannelEvent, Command, LinkInfo, PeerConfig, PeerError, PeerOutput, PeerSnapshot, SessionEvent,
	peer_id,
};
use super::link::{Established, LinkEvent, LinkId, LinkParams};
use super::sessions::SessionTable;
use super::signaling::{Inbound, LinkNet, decode};
use super::trust::{SharedTrust, now};

const PENDING_PER_PEER: usize = 32;
/// Discovery records not refreshed within this long are forgotten (longer than the mDNS refresh).
const DISCOVERY_TTL: std::time::Duration = std::time::Duration::from_secs(120);
/// Period of the manager's own upkeep.
const UPKEEP: std::time::Duration = std::time::Duration::from_secs(5);

enum Outbox {
	Sent,
	Full,
	Closed,
}

pub(super) struct Entry {
	pub(super) link: Established,
	pub(super) trusted: bool,
}

pub(super) struct Actor {
	pub(super) config: PeerConfig,
	pub(super) local: Fingerprint,
	pub(super) params: Arc<LinkParams>,
	pub(super) trust: SharedTrust,
	pub(super) links: HashMap<LinkId, Entry>,
	pub(super) canonical: HashMap<Fingerprint, LinkId>,
	pub(super) wanted: HashMap<Fingerprint, Want>,
	pub(super) dialling: HashMap<LinkId, Option<Fingerprint>>,
	pub(super) addresses: HashMap<Fingerprint, Vec<SocketAddr>>,
	pub(super) table: DiscoveryTable,
	pub(super) probed: super::probing::Probed,
	/// Authenticated links nobody trusts yet, bounded in total and per source.
	pub(super) untrusted: UntrustedLinks<LinkId>,
	pub(super) pending: HashMap<Fingerprint, VecDeque<Envelope>>,
	pub(super) sessions: Arc<Mutex<SessionTable>>,
	pub(super) waiters: HashMap<LinkId, oneshot::Sender<Result<Fingerprint, PeerError>>>,
	pub(super) ids: Arc<AtomicU64>,
	pub(super) output: mpsc::Sender<PeerOutput>,
	pub(super) link_tx: mpsc::Sender<LinkEvent>,
	pub(super) commands: mpsc::Sender<Command>,
	pub(super) snapshot: Arc<ArcSwap<Vec<PeerSnapshot>>>,
	pub(super) accept: tokio::task::AbortHandle,
	pub(super) profiles: super::profile::Profiles,
	pub(super) remote_profiles: HashMap<Fingerprint, (String, Option<[u8; 32]>)>,
	pub(super) profile_tx: tokio::sync::watch::Sender<super::profile::LocalProfile>,
}

impl Actor {
	pub(super) async fn run(
		mut self,
		mut commands: mpsc::Receiver<Command>,
		mut links: mpsc::Receiver<LinkEvent>,
	) {
		let mut upkeep = tokio::time::interval(UPKEEP);
		upkeep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
		loop {
			tokio::select! {
				_ = upkeep.tick() => self.upkeep(),
				command = commands.recv() => match command {
					Some(Command::Shutdown) | None => break,
					Some(command) => self.on_command(command).await,
				},
				Some(event) = links.recv() => self.on_link_event(event).await,
			}
			self.publish_snapshot();
		}
		// Dropping the links aborts their tasks; stop accepting new ones too.
		self.accept.abort();
	}

	/// Forgets discovery records that stopped refreshing, whether or not a goodbye came.
	fn upkeep(&mut self) {
		self.table.expire(DISCOVERY_TTL);
		self.table.take_evicted();
		self.probe_discovered();
	}

	pub(super) async fn emit(&self, output: PeerOutput) {
		let _ = self.output.send(output).await;
	}

	async fn on_command(&mut self, command: Command) {
		match command {
			Command::Ensure(peer) => self.ensure(peer).await,
			Command::Forgotten(peer) => self.forgotten(peer).await,
			Command::Release(peer) => {
				self.wanted.remove(&peer);
				self.pending.remove(&peer);
			}
			Command::Dial {
				addr,
				expected,
				reply,
			} => {
				if let Err(error) = self.config.manual_policy().check_manual_socket(addr) {
					let _ = reply.send(Err(error.into()));
					return;
				}
				let id = self.dial(vec![addr], expected);
				self.waiters.insert(id, reply);
			}
			Command::Send(peer, envelope) => self.send(peer, envelope).await,
			Command::Channel(peer, message) => self.send_channel(peer, *message),
			Command::Session(peer, message) => self.send_session(peer, *message),
			Command::Pin { peer, state, reply } => {
				let result = self.pin(peer, state).await;
				let _ = reply.send(result);
			}
			Command::Link { peer, reply } => {
				let info = self.trusted_link(&peer).map(|entry| LinkInfo {
					session: Arc::clone(&entry.link.session),
					local_ip: entry.link.local_ip,
					overlay_peer: self.config.overlay_peer(entry.link.remote_ip),
				});
				let _ = reply.send(info);
			}
			Command::AddAddress(peer, addr) => {
				if self
					.config
					.manual_policy()
					.check_manual_socket(addr)
					.is_ok()
				{
					self.remember_address(peer, addr);
					self.try_dial(peer);
				}
			}
			Command::SetProfile(profile) => self.set_profile(profile).await,
			Command::AvatarValidated {
				peer,
				expected,
				stored,
			} => self.avatar_validated(peer, expected, stored).await,
			Command::AvatarServed(peer) => self.profiles.finish_serving(&peer),
			Command::Redial(peer) => {
				if let Some(want) = self.wanted.get_mut(&peer) {
					want.retry_scheduled = false;
					self.try_dial(peer);
				}
			}
			Command::Discovered(found) => self.on_discovered(*found),
			Command::Removed(name) => {
				self.table.remove(&name);
			}
			Command::Visibility => self.probe_discovered(),
			Command::Shutdown => {}
		}
	}

	/// Channel exchanges belong to one session: sent on the live trusted link
	/// or not at all.
	fn send_channel(&mut self, peer: Fingerprint, message: relay_net::proto::ChannelMessage) {
		if let Some(entry) = self.trusted_link(&peer) {
			let envelope = relay_net::control::new_envelope(Body::Channel(message));
			let _ = entry.link.outbox.try_send(envelope);
		}
	}

	/// Session messages go on the live trusted link or nowhere; a full queue drops the
	/// message (it is retried by its owner) instead of tearing the link down.
	fn send_session(&mut self, peer: Fingerprint, message: relay_net::proto::SessionMessage) {
		if let Some(entry) = self.trusted_link(&peer) {
			let envelope = relay_net::control::new_envelope(Body::SessionMessage(message));
			let _ = entry.link.outbox.try_send(envelope);
		}
	}

	fn on_discovered(&mut self, mut found: relay_net::discovery::DiscoveredPeer) {
		found.display_name = relay_net::discovery::sanitize_display_name(&found.display_name);
		// Whatever feeds the table, only plain-policy addresses are ever dialled.
		found
			.addresses
			.retain(|addr| self.config.policy.check_socket(*addr).is_ok());
		let matches: Vec<Fingerprint> = self
			.wanted
			.keys()
			.filter(|fp| found.fingerprint_hint.is_some_and(|hint| hint.matches(fp)))
			.copied()
			.collect();
		let hints = found.clone();
		let stored = self.table.upsert(found).is_ok();
		self.table.take_evicted();
		if !stored {
			return;
		}
		// The channel driver recognises blinded tags; they are hints, never proof.
		let _ = self
			.output
			.try_send(PeerOutput::Channel(ChannelEvent::Advert(Box::new(hints))));
		matches.into_iter().for_each(|peer| self.try_dial(peer));
		self.probe_discovered();
	}

	pub(super) fn trusted_link(&self, peer: &Fingerprint) -> Option<&Entry> {
		let entry = self.links.get(self.canonical.get(peer)?)?;
		entry.trusted.then_some(entry)
	}

	async fn ensure(&mut self, peer: Fingerprint) {
		self.wanted.entry(peer).or_default();
		if self.trusted_link(&peer).is_some() {
			self.emit(PeerOutput::Core(PeerEvent::LinkUp(peer_id(&peer))))
				.await;
		} else {
			self.try_dial(peer);
		}
	}

	pub(super) async fn send(&mut self, peer: Fingerprint, envelope: Envelope) {
		let outcome = self.trusted_link(&peer).map(|entry| {
			match entry.link.outbox.try_send(envelope.clone()) {
				Ok(()) => Outbox::Sent,
				Err(mpsc::error::TrySendError::Full(_)) => Outbox::Full,
				Err(mpsc::error::TrySendError::Closed(_)) => Outbox::Closed,
			}
		});
		match outcome {
			Some(Outbox::Sent | Outbox::Closed) => {}
			Some(Outbox::Full) => {
				// A control queue that cannot drain means a dead or hostile peer.
				if let Some(id) = self.canonical.get(&peer).copied() {
					self.drop_link(id).await;
				}
			}
			None => {
				let queue = self.pending.entry(peer).or_default();
				if queue.len() >= PENDING_PER_PEER {
					queue.pop_front();
				}
				queue.push_back(envelope);
			}
		}
	}

	async fn flush_pending(&mut self, peer: Fingerprint) {
		let Some(queue) = self.pending.remove(&peer) else {
			return;
		};
		for envelope in queue {
			self.send(peer, envelope).await;
		}
	}

	async fn pin(&mut self, peer: Fingerprint, state: TrustState) -> Result<(), PeerError> {
		let (name, key) = self
			.canonical
			.get(&peer)
			.and_then(|id| self.links.get(id))
			.map(|entry| (entry.link.name.clone(), entry.link.key))
			.ok_or(PeerError::UnknownKey)?;
		// The held link carries the key, so identity churn cannot evict it from the
		// store's bounded cache before the user decides.
		self.trust.lock().observe(&key);
		self.trust
			.lock()
			.put(TrustRecord {
				fingerprint: peer,
				state,
				display_name: name,
				trusted_at: now(),
			})
			.map_err(|error| match error {
				relay_net::identity::TrustError::Unknown => PeerError::UnknownKey,
				other => PeerError::Trust(other.to_string()),
			})?;
		let promoted = self
			.canonical
			.get(&peer)
			.and_then(|id| self.links.get_mut(id))
			.is_some_and(|entry| !std::mem::replace(&mut entry.trusted, true));
		if promoted {
			if let Some(id) = self.canonical.get(&peer) {
				self.untrusted.release(id);
			}
			self.emit(PeerOutput::Core(PeerEvent::LinkUp(peer_id(&peer))))
				.await;
			self.flush_pending(peer).await;
			self.announce_profile(peer).await;
			self.announce_channel_link(peer).await;
			self.announce_session_link(peer).await;
		}
		Ok(())
	}

	async fn on_link_event(&mut self, event: LinkEvent) {
		match event {
			LinkEvent::Up(link) => self.on_up(*link).await,
			LinkEvent::Message { link, body } => self.on_message(link, body).await,
			LinkEvent::Down { link } => self.drop_link(link).await,
			LinkEvent::Failed { link, error } => {
				tracing::debug!(link, %error, "link attempt failed");
				if let Some(waiter) = self.waiters.remove(&link) {
					let _ = waiter.send(Err(PeerError::Discovery(error)));
				}
				if let Some(Some(peer)) = self.dialling.remove(&link) {
					self.dial_ended(peer);
				}
			}
		}
	}

	async fn on_up(&mut self, link: Established) {
		let peer = link.fingerprint;
		self.dialling.remove(&link.id);
		self.trust.lock().observe(&link.key);
		let trust = evaluate_peer(&*self.trust.lock(), &peer, &link.name);
		if let PeerTrust::KeyChanged { previous } = &trust {
			let output = PeerOutput::KeyChanged {
				peer: peer_id(&peer),
				name: link.name.clone(),
				previous: peer_id(&previous.fingerprint),
			};
			if let Some(waiter) = self.waiters.remove(&link.id) {
				let _ = waiter.send(Err(PeerError::Trust("key changed".to_owned())));
			}
			self.emit(output).await;
			return;
		}
		if let Some(waiter) = self.waiters.remove(&link.id) {
			let _ = waiter.send(Ok(peer));
		}
		let trusted = matches!(trust, PeerTrust::Verified | PeerTrust::UnverifiedFirstUse);
		let mut previously_up = false;
		if let Some(old_id) = self.canonical.get(&peer).copied() {
			let old = &self.links[&old_id];
			let facts = |l: &Established| LinkFacts {
				dialled_locally: l.dialled_locally,
				epoch: l.epoch,
			};
			if elect(&self.local, &peer, facts(&old.link), facts(&link)) == Verdict::KeepFirst {
				return;
			}
			previously_up = old.trusted;
			self.links.remove(&old_id);
			self.untrusted.release(&old_id);
		}
		self.canonical.insert(peer, link.id);
		let name = self.shown_name(&peer, &link.name);
		let (id, source) = (link.id, link.remote_ip);
		self.links.insert(link.id, Entry { link, trusted });
		if !trusted {
			for victim in self.untrusted.admit(id, source) {
				self.drop_link(victim).await;
			}
		}
		if let Some(want) = self.wanted.get_mut(&peer) {
			want.attempt = 0;
			want.dialling = false;
		}
		if trusted {
			if !previously_up {
				self.emit(PeerOutput::Core(PeerEvent::LinkUp(peer_id(&peer))))
					.await;
			}
			self.flush_pending(peer).await;
			self.announce_profile(peer).await;
			self.announce_channel_link(peer).await;
			self.announce_session_link(peer).await;
		} else {
			self.emit(PeerOutput::TrustRequired {
				peer: peer_id(&peer),
				name,
				trust,
			})
			.await;
		}
	}

	/// Tells the channel driver that a trusted link is live. Trust was evaluated
	/// before this point; the driver starts no exchange earlier.
	async fn announce_channel_link(&self, peer: Fingerprint) {
		let Some(entry) = self.trusted_link(&peer) else {
			return;
		};
		let event = ChannelEvent::LinkUp {
			peer: peer_id(&peer),
			binding: SessionBinding::for_session(&entry.link.session),
		};
		self.emit(PeerOutput::Channel(event)).await;
	}

	/// Tells the messaging actor where a trusted link comes from.
	async fn announce_session_link(&self, peer: Fingerprint) {
		let Some(entry) = self.trusted_link(&peer) else {
			return;
		};
		let event = SessionEvent::LinkUp {
			peer: peer_id(&peer),
			remote_ip: entry.link.remote_ip,
		};
		self.emit(PeerOutput::Session(event)).await;
	}

	async fn on_message(&mut self, link: LinkId, body: Body) {
		let Some(entry) = self.links.get(&link) else {
			return;
		};
		// Only the current canonical, trusted link may speak to the domain.
		if !entry.trusted || self.canonical.get(&entry.link.fingerprint) != Some(&link) {
			return;
		}
		let fingerprint = entry.link.fingerprint;
		if let Body::Channel(message) = body {
			let event = ChannelEvent::Message {
				peer: peer_id(&fingerprint),
				binding: SessionBinding::for_session(&entry.link.session),
				message: Box::new(message),
			};
			self.emit(PeerOutput::Channel(event)).await;
			return;
		}
		if let Body::SessionMessage(message) = body {
			// The sender is the authenticated owner of this link, never a field of the message.
			let event = SessionEvent::Message {
				peer: peer_id(&fingerprint),
				message: Box::new(message),
			};
			self.emit(PeerOutput::Session(event)).await;
			return;
		}
		if matches!(
			body,
			Body::Profile(_) | Body::AvatarRequest(_) | Body::AvatarChunk(_)
		) {
			self.on_profile_body(fingerprint, body).await;
			return;
		}
		let peer = peer_id(&fingerprint);
		let inbound = {
			let mut table = self.sessions.lock_unpoisoned();
			let net = LinkNet {
				remote_ip: entry.link.remote_ip,
				subnet: entry.link.subnet,
			};
			let policy = self.config.link_policy(entry.link.remote_ip);
			decode(peer, body, &mut table, &policy, &net)
		};
		match inbound {
			Inbound::Event(event) => self.emit(PeerOutput::Core(event)).await,
			Inbound::Media(signal) => self.emit(PeerOutput::Media { peer, signal }).await,
			Inbound::Ignored => {}
		}
	}

	/// Removes a link; reports the peer down only if it was the live one.
	pub(super) async fn drop_link(&mut self, id: LinkId) {
		self.untrusted.release(&id);
		let Some(entry) = self.links.remove(&id) else {
			return;
		};
		let peer = entry.link.fingerprint;
		if self.canonical.get(&peer) != Some(&id) {
			return;
		}
		self.canonical.remove(&peer);
		self.profiles.forget(&peer);
		if entry.trusted {
			self.emit(PeerOutput::Core(PeerEvent::LinkDown(peer_id(&peer))))
				.await;
			let down = ChannelEvent::LinkDown {
				peer: peer_id(&peer),
			};
			self.emit(PeerOutput::Channel(down)).await;
			let down = SessionEvent::LinkDown {
				peer: peer_id(&peer),
			};
			self.emit(PeerOutput::Session(down)).await;
		}
		self.schedule_redial(peer);
	}

	fn publish_snapshot(&self) {
		let mut list: Vec<PeerSnapshot> = Vec::new();
		for (peer, id) in &self.canonical {
			let entry = &self.links[id];
			let (name, avatar) = self
				.remote_profiles
				.get(peer)
				.cloned()
				.unwrap_or_else(|| (self.shown_name(peer, &entry.link.name), None));
			list.push(PeerSnapshot {
				id: Some(peer_id(peer)),
				hint: None,
				avatar,
				name,
				addresses: self.addresses.get(peer).cloned().unwrap_or_default(),
				linked: entry.trusted,
				trust: self.trust.lock().get(peer).map(|r| r.state),
				probe: Some(Arc::clone(&entry.link.rtt)),
			});
		}
		// Discovered devices without a link: name and hint only, never trusted.
		for found in self.table.iter() {
			let linked = found
				.fingerprint_hint
				.is_some_and(|hint| self.canonical.keys().any(|fp| hint.matches(fp)));
			if !linked {
				list.push(PeerSnapshot {
					id: None,
					hint: found.fingerprint_hint,
					avatar: None,
					name: found.display_name.clone(),
					addresses: found.addresses.clone(),
					linked: false,
					trust: None,
					probe: None,
				});
			}
		}
		self.snapshot.store(Arc::new(list));
	}
}
