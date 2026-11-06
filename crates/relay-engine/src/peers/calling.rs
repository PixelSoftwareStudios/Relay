//! Calls with devices nobody has trusted yet.
//!
//! An untrusted link may carry exactly four things: the invitation, its acceptance, a
//! refusal and a hang-up. Nothing else (channels, messages, profiles, media negotiation)
//! flows until the key is pinned. The pin happens when the call is answered, on the very
//! link that carried the invitation, and only with the key that link's handshake showed:
//!
//! - the callee pins when it sends `Accept`;
//! - the caller pins when `Accept` arrives for its call on the link it invited over.
//!
//! A declined, cancelled or unanswered call pins nothing. The pin is first-use trust
//! (`UnverifiedFirstUse`, never `Verified`). A device that is not visible (private mode, no
//! client attached) takes no calls from strangers at all.

use std::collections::{HashMap, VecDeque};

use relay_core::PeerEvent;
use relay_net::identity::{Fingerprint, TrustState};
use relay_net::proto::{Envelope, envelope::Body};

use super::link::LinkId;
use super::manager::Actor;
use super::{PeerOutput, peer_id};

/// Most calls whose carrying link is remembered at once.
const CALLS_MAX: usize = 64;

/// Which untrusted link carried each call's invitation, as raw call id bytes.
#[derive(Default)]
pub(super) struct CallLinks {
	links: HashMap<[u8; 16], LinkId>,
	order: VecDeque<[u8; 16]>,
}

impl CallLinks {
	fn record(&mut self, call: [u8; 16], link: LinkId) {
		if self.links.insert(call, link).is_none() {
			self.order.push_back(call);
		}
		while self.order.len() > CALLS_MAX {
			if let Some(old) = self.order.pop_front() {
				self.links.remove(&old);
			}
		}
	}

	fn get(&self, call: &[u8; 16]) -> Option<LinkId> {
		self.links.get(call).copied()
	}

	fn forget(&mut self, call: &[u8; 16]) {
		self.links.remove(call);
		self.order.retain(|c| c != call);
	}
}

fn raw_call(bytes: &[u8]) -> Option<[u8; 16]> {
	bytes.try_into().ok()
}

impl Actor {
	/// True while strangers may reach this device for a call: a client is attached and the
	/// user has not gone private (the same switches that decide whether it is advertised).
	pub(super) fn open_to_strangers(&self) -> bool {
		self.config
			.advert
			.as_ref()
			.is_some_and(|spec| spec.borrow().present && !spec.borrow().hidden)
	}

	/// Sends call signalling on the untrusted link of `peer`. Returns false when this
	/// envelope is not call signalling or no such link exists (the caller then queues it).
	pub(super) async fn send_untrusted(&mut self, peer: Fingerprint, envelope: &Envelope) -> bool {
		let Some(id) = self.canonical.get(&peer).copied() else {
			return false;
		};
		let Some(entry) = self.links.get(&id).filter(|e| !e.trusted) else {
			return false;
		};
		let outbox = entry.link.outbox.clone();
		match envelope.body.as_ref() {
			Some(Body::Invite(m)) => {
				let Some(call) = raw_call(&m.call_id) else {
					return true;
				};
				self.call_links.record(call, id);
			}
			Some(Body::Accept(m)) => {
				// Accepting is the moment the callee trusts the caller: only on the link
				// that brought the invitation, and with that link's own key.
				let Some(call) = raw_call(&m.call_id) else {
					return true;
				};
				if self.call_links.get(&call) != Some(id) {
					tracing::debug!("accept for a call that arrived on another link: dropped");
					return true;
				}
				if let Err(error) = Box::pin(self.pin(peer, TrustState::UnverifiedFirstUse)).await {
					tracing::warn!(%error, "could not trust the caller; accept not sent");
					return true;
				}
				self.call_links.forget(&call);
				return self.send_on(peer, envelope.clone()).await;
			}
			Some(Body::Reject(m)) => {
				if let Some(call) = raw_call(&m.call_id) {
					self.call_links.forget(&call);
				}
			}
			Some(Body::End(m)) => {
				if let Some(call) = raw_call(&m.call_id) {
					self.call_links.forget(&call);
				}
			}
			_ => return false,
		}
		let _ = outbox.try_send(envelope.clone());
		true
	}

	/// Puts an envelope on the (now trusted) link of `peer`.
	async fn send_on(&mut self, peer: Fingerprint, envelope: Envelope) -> bool {
		if let Some(entry) = self.trusted_link(&peer) {
			let _ = entry.link.outbox.try_send(envelope);
		}
		true
	}

	/// Whether an inbound body may be acted on from an untrusted link.
	pub(super) fn call_signalling_allowed(&self, body: &Body) -> bool {
		matches!(
			body,
			Body::Invite(_) | Body::Accept(_) | Body::Reject(_) | Body::End(_)
		) && self.open_to_strangers()
	}

	/// Handles the domain event that an untrusted link's message decoded to. Returns
	/// whether the event may continue to the domain.
	pub(super) async fn screen_untrusted(
		&mut self,
		link: LinkId,
		peer: Fingerprint,
		event: &PeerEvent,
	) -> bool {
		match event {
			PeerEvent::Invite { call, .. } => {
				self.call_links.record(call.to_bytes(), link);
				true
			}
			PeerEvent::Accepted { call, .. } => {
				let bytes = call.to_bytes();
				if self.call_links.get(&bytes) != Some(link) {
					return false;
				}
				// The caller trusts the callee now: the call was answered on this link.
				if let Err(error) = Box::pin(self.pin(peer, TrustState::UnverifiedFirstUse)).await {
					tracing::warn!(%error, "could not trust the callee; accept ignored");
					return false;
				}
				self.call_links.forget(&bytes);
				true
			}
			PeerEvent::Declined { call, .. } | PeerEvent::Ended { call, .. } => {
				self.call_links.forget(&call.to_bytes());
				true
			}
			_ => false,
		}
	}

	/// Tells the domain a stranger's link is usable for a call, when strangers may call.
	pub(super) async fn announce_call_link(&mut self, peer: Fingerprint) {
		if !self.open_to_strangers() {
			return;
		}
		let announce = self
			.canonical
			.get(&peer)
			.and_then(|id| self.links.get_mut(id))
			.is_some_and(|entry| !std::mem::replace(&mut entry.domain_up, true));
		if announce {
			self.emit(PeerOutput::Core(PeerEvent::LinkUp(peer_id(&peer))))
				.await;
		}
	}
}
