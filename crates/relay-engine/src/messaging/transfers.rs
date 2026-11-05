//! Transfer bookkeeping in the actor: who runs, who waits for a grant, and how a task's
//! end becomes a transcript status.

use std::net::SocketAddr;
use std::time::Duration;

use relay_core::PeerId;
use relay_messages::transfer::CancelBy;
use relay_messages::{MessageBody, OfferId, OfferStatus, SessionId};
use relay_net::identity::Fingerprint;
use relay_net::proto::CancelReason;
use tokio::sync::mpsc;

use super::hub::Hub;
use super::session::{XferCmd, failure_label};
use super::wire;
use super::xfer::{Outcome, SendJob, TaskCtx, run_send};

/// How long a sender waits for the receiver's grant after it accepted.
const GRANT_WAIT: Duration = Duration::from_secs(35);

impl Hub {
	pub(super) fn offer_status(&self, sid: SessionId, id: OfferId) -> Option<OfferStatus> {
		self.history
			.get(sid)?
			.messages()
			.iter()
			.find_map(|m| match &m.body {
				MessageBody::File(o) if o.id == id => Some(o.status),
				_ => None,
			})
	}

	pub(super) fn set_offer(&mut self, sid: SessionId, id: OfferId, status: OfferStatus) {
		if let Some(store) = self.history.get_mut(sid) {
			let _ = store.set_offer_status(id, status);
		}
		self.emit_offer(sid, id);
	}

	/// Ends an offer without a task: records why and tells watchers.
	pub(super) fn fail_offer(
		&mut self,
		sid: SessionId,
		id: OfferId,
		status: OfferStatus,
		label: &str,
	) {
		if let Some(x) = self
			.sessions
			.get_mut(&sid)
			.and_then(|m| m.xfers.get_mut(&id))
		{
			x.failure = label.to_owned();
			x.active = false;
			x.task = None;
		}
		self.set_offer(sid, id, status);
	}

	/// Transfers counting toward the per-session limit.
	pub(super) fn active_transfers(&self, sid: SessionId) -> usize {
		self.sessions
			.get(&sid)
			.map_or(0, |m| m.xfers.values().filter(|x| x.active).count())
	}

	/// The sender waits for the receiver's grant; give up if it never comes.
	pub(super) fn expect_grant(&mut self, sid: SessionId, id: OfferId) {
		let handle = self.handle.clone();
		tokio::spawn(async move {
			tokio::time::sleep(GRANT_WAIT).await;
			let _ = handle.post(move |hub| hub.grant_timeout(sid, id)).await;
		});
	}

	fn grant_timeout(&mut self, sid: SessionId, id: OfferId) {
		let waiting = self
			.sessions
			.get(&sid)
			.and_then(|m| m.xfers.get(&id))
			.is_some_and(|x| x.accepted && x.task.is_none() && x.active);
		if waiting && self.offer_status(sid, id) == Some(OfferStatus::InProgress) {
			self.fail_offer(sid, id, OfferStatus::Failed, "no_data_link");
		}
	}

	/// The receiver granted a data link: dial it.
	pub(super) fn on_grant(
		&mut self,
		sid: SessionId,
		peer: PeerId,
		id: OfferId,
		token: [u8; 32],
		port: u16,
	) {
		let ready = self
			.sessions
			.get(&sid)
			.and_then(|m| m.xfers.get(&id))
			.is_some_and(|x| x.outgoing && x.accepted && x.task.is_none() && x.source.is_some())
			&& self.offer_status(sid, id) == Some(OfferStatus::InProgress);
		if !ready {
			return;
		}
		let (Some(hub), Some(ip)) = (self.env.transfers.clone(), self.links.get(&peer).copied())
		else {
			self.fail_offer(sid, id, OfferStatus::Failed, "no_data_link");
			return;
		};
		let offer = self.history.get(sid).and_then(|s| {
			s.messages().iter().find_map(|m| match &m.body {
				MessageBody::File(o) if o.id == id => Some(o.clone()),
				_ => None,
			})
		});
		let Some(meta) = self.sessions.get_mut(&sid) else {
			return;
		};
		let (Some(offer), Some(source)) =
			(offer, meta.xfers.get(&id).and_then(|x| x.source.clone()))
		else {
			return;
		};
		let (tx, cmds) = mpsc::channel(8);
		if let Some(x) = meta.xfers.get_mut(&id) {
			x.task = Some(tx);
		}
		let job = SendJob {
			ctx: TaskCtx {
				handle: self.handle.clone(),
				session: sid,
				offer: id,
			},
			hub,
			peer: Fingerprint::from_bytes(*peer.as_bytes()),
			addr: SocketAddr::new(ip, port),
			session_id: meta.call.to_bytes(),
			token,
			offer,
			source,
			cmds,
			rate: self.rate.subscribe(),
		};
		tokio::spawn(run_send(job));
	}

	/// Stops every transfer of a session: running tasks are told to cancel, a transfer
	/// that waits for its grant ends at once.
	pub(super) fn cancel_session_transfers(&mut self, sid: SessionId, ending: bool) {
		let Some(meta) = self.sessions.get(&sid) else {
			return;
		};
		let peer = meta.peer;
		let targets: Vec<(OfferId, Option<mpsc::Sender<XferCmd>>)> = meta
			.xfers
			.iter()
			.filter(|(_, x)| x.active)
			.map(|(id, x)| (*id, x.task.clone()))
			.collect();
		for (id, task) in targets {
			match task {
				Some(task) => {
					let _ = task.try_send(XferCmd::Cancel);
				}
				None => {
					if !ending {
						self.send(peer, wire::cancel(id, CancelReason::CancelDisabled));
					}
					self.fail_offer(sid, id, OfferStatus::Cancelled, "cancelled");
				}
			}
		}
	}

	pub(super) fn xfer_progress(&mut self, sid: SessionId, id: OfferId, bytes: u64) {
		let Some(x) = self
			.sessions
			.get_mut(&sid)
			.and_then(|m| m.xfers.get_mut(&id))
		else {
			return;
		};
		x.transferred = bytes;
		x.last_reported = bytes;
		self.emit_offer(sid, id);
	}

	/// A task ended: record the result and tell watchers.
	pub(super) fn xfer_finished(&mut self, sid: SessionId, id: OfferId, outcome: Outcome) {
		let Some(meta) = self.sessions.get_mut(&sid) else {
			return;
		};
		let peer = meta.peer;
		let Some(x) = meta.xfers.get_mut(&id) else {
			return;
		};
		x.active = false;
		x.task = None;
		let status = match &outcome {
			Outcome::Completed { saved_as } => {
				if let Some(name) = saved_as {
					x.saved_as.clone_from(name);
				}
				OfferStatus::Completed
			}
			Outcome::Failed(why) => {
				x.failure = failure_label(*why).to_owned();
				OfferStatus::Failed
			}
			Outcome::Cancelled(by) => {
				x.failure = match by {
					CancelBy::Local => "cancelled",
					CancelBy::Peer => "cancelled_by_peer",
				}
				.to_owned();
				OfferStatus::Cancelled
			}
		};
		let session_live = self.by_peer.get(&peer) == Some(&sid);
		if session_live {
			match &outcome {
				Outcome::Cancelled(CancelBy::Local) => {
					self.send(peer, wire::cancel(id, CancelReason::CancelUser));
				}
				Outcome::Failed(_) => {
					self.send(peer, wire::cancel(id, CancelReason::CancelError));
				}
				_ => {}
			}
		}
		self.set_offer(sid, id, status);
	}
}
