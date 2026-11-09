//! Channel events from the peer manager: link lifecycle and protocol messages.

use relay_core::PeerId;
use relay_net::channel::ids::channel_id;
use relay_net::channel::join::apply_renewal;
use relay_net::channel::{Begin, SessionBinding};
use relay_net::proto::ChannelMessage;
use relay_net::proto::channel_message::Body;

use super::failure::Failure;
use super::hub::ChannelHub;
use super::state::{JoinSlot, Link};
use crate::peers::{ChannelEvent, fingerprint};

/// Work decided under the state lock and done after it is released.
enum Next {
	Nothing,
	Send(ChannelMessage),
	Persist {
		slot: JoinSlot,
		grant: ChannelMessage,
	},
	Renewed(ChannelMessage),
	Fail(JoinSlot),
}

impl ChannelHub {
	/// Handles one event from the peer manager. These never reach call state.
	pub async fn on_event(&self, event: ChannelEvent) {
		match event {
			ChannelEvent::LinkUp { peer, binding } => self.link_up(peer, binding).await,
			ChannelEvent::LinkDown { peer } => self.link_down(peer).await,
			ChannelEvent::Message {
				peer,
				binding,
				message,
			} => self.message(peer, &binding, &message).await,
			ChannelEvent::Advert(found) => {
				if self.on_advert(&found) {
					self.publish().await;
				}
			}
		}
	}

	async fn link_up(&self, peer: PeerId, binding: SessionBinding) {
		let replaced = {
			let mut state = self.state();
			state.forget_peer(&peer);
			state.links.insert(peer, Link::new(binding))
		};
		if let Some(old) = replaced.and_then(|l| l.join) {
			let _ = old.done.send(Err(Failure::Denied));
		}
		self.send_refs(peer, None);
		self.publish().await;
	}

	async fn link_down(&self, peer: PeerId) {
		let gone = {
			let mut state = self.state();
			state.forget_peer(&peer);
			state.links.remove(&peer)
		};
		if let Some(slot) = gone.and_then(|l| l.join) {
			let _ = slot.done.send(Err(Failure::Denied));
		}
		self.publish().await;
	}

	async fn message(&self, peer: PeerId, binding: &SessionBinding, message: &ChannelMessage) {
		// A message from a session that is no longer the live one is ignored.
		if self.state().links.get(&peer).map(|l| l.binding) != Some(*binding) {
			return;
		}
		let Some(body) = &message.body else { return };
		// A link may not make us verify, hash or store without bound.
		let mono = self.mono();
		let allowed = self
			.state()
			.links
			.get_mut(&peer)
			.is_some_and(|l| l.budget.allow(mono));
		if !allowed {
			tracing::debug!("channel message dropped: link over its rate");
			return;
		}
		let changed = match body {
			Body::MemberRef(_) => {
				self.on_member_ref(peer, message);
				false
			}
			Body::Proof(_) => self.on_proof(peer, message),
			Body::JoinStart(_) => {
				self.admit_begin(peer, binding, message);
				false
			}
			Body::JoinConfirm(_) => {
				self.admit_confirm(peer, message);
				false
			}
			Body::Renew(_) => {
				self.admit_renew(peer, binding, message);
				false
			}
			Body::JoinReply(_) | Body::JoinGrant(_) | Body::JoinDenied(_) => {
				self.joiner_side(peer, message).await
			}
		};
		if changed {
			self.publish().await;
		}
	}

	fn admit_begin(&self, peer: PeerId, binding: &SessionBinding, message: &ChannelMessage) {
		let (mono, now) = (self.mono(), self.now());
		let known = self.state().links.contains_key(&peer);
		let reply = if !known {
			denied()
		} else {
			// One admission exchange per link: a new start replaces an abandoned one.
			if let Some(link) = self.state().links.get_mut(&peer) {
				link.admit = None;
			}
			match self
				.inner
				.admission
				.begin(binding, &fingerprint(peer), message, mono, now)
			{
				Begin::Reply(session, reply) => {
					if let Some(link) = self.state().links.get_mut(&peer) {
						link.admit = Some((*session, mono));
					}
					reply
				}
				Begin::Denied(refusal) => refusal,
			}
		};
		self.send(peer, reply);
	}

	fn admit_confirm(&self, peer: PeerId, message: &ChannelMessage) {
		let session = self
			.state()
			.links
			.get_mut(&peer)
			.and_then(|l| l.admit.take());
		if let Some((session, _)) = session {
			self.send(peer, session.on_confirm(message, self.now()));
		}
	}

	fn admit_renew(&self, peer: PeerId, binding: &SessionBinding, message: &ChannelMessage) {
		let reply = self.inner.admission.renew(
			binding,
			&fingerprint(peer),
			message,
			self.mono(),
			self.now(),
		);
		self.send(peer, reply);
	}

	/// Replies to a join we started, or to a renewal we asked for.
	async fn joiner_side(&self, peer: PeerId, message: &ChannelMessage) -> bool {
		let next = {
			let mut state = self.state();
			let Some(link) = state.links.get_mut(&peer) else {
				return false;
			};
			next_step(link, message)
		};
		match next {
			Next::Nothing => false,
			Next::Send(reply) => {
				self.send(peer, reply);
				false
			}
			Next::Fail(slot) => {
				let _ = slot.done.send(Err(Failure::Denied));
				false
			}
			Next::Persist { slot, grant } => {
				let result = self.accept_grant(slot.client, &grant);
				let ok = result.is_ok();
				let _ = slot.done.send(result);
				ok
			}
			Next::Renewed(grant) => self.finish_renewal(peer, &grant),
		}
	}

	fn finish_renewal(&self, peer: PeerId, grant: &ChannelMessage) -> bool {
		let Some(Body::JoinGrant(g)) = &grant.body else {
			return false;
		};
		let Ok(owner) = <[u8; 32]>::try_from(g.owner_public.as_slice()) else {
			return false;
		};
		let id = channel_id(&owner);
		let asked = self
			.state()
			.links
			.get_mut(&peer)
			.is_some_and(|l| l.renew.remove(&id));
		if !asked {
			return false;
		}
		let (now, local) = (self.now(), self.inner.local);
		let mut applied = false;
		let _ = self.inner.store.update(&id, &mut |record| {
			applied = apply_renewal(record, grant, &local, now).is_ok();
		});
		if applied {
			let _ = self.reload();
		}
		applied
	}
}

fn denied() -> ChannelMessage {
	ChannelMessage {
		body: Some(Body::JoinDenied(relay_net::proto::ChannelJoinDenied {})),
	}
}

fn next_step(link: &mut Link, message: &ChannelMessage) -> Next {
	match &message.body {
		Some(Body::JoinReply(_)) => {
			let Some(slot) = &mut link.join else {
				return Next::Nothing;
			};
			match slot.client.on_reply(message) {
				Ok(confirm) => Next::Send(confirm),
				Err(_) => link.join.take().map_or(Next::Nothing, Next::Fail),
			}
		}
		Some(Body::JoinGrant(_)) => match link.join.take() {
			Some(slot) => Next::Persist {
				slot,
				grant: message.clone(),
			},
			None if !link.renew.is_empty() => Next::Renewed(message.clone()),
			None => Next::Nothing,
		},
		Some(Body::JoinDenied(_)) => match link.join.take() {
			Some(slot) => Next::Fail(slot),
			None => {
				link.renew.clear();
				Next::Nothing
			}
		},
		_ => Next::Nothing,
	}
}
