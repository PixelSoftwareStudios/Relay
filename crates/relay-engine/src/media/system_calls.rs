//! Activation and retirement of a call's media, and the remote's negotiation.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use relay_core::{CallId, PeerEvent, PeerId, SessionId, SystemEvent};

use super::capture_worker::CaptureCmd;
use super::devices::DeviceCmd;
use super::events::MediaEvent;
use super::recv::{CallReceiver, RemoteStream};
use super::render_worker::RenderCmd;
use super::state::{Phase, RemoteReady};
use super::system::MediaSystem;
use crate::peers::MediaSignal;

impl MediaSystem {
	/// Feeds the remote's media negotiation message into the system.
	pub fn handle_signal(&self, peer: PeerId, signal: MediaSignal) {
		match signal {
			MediaSignal::Offer { epoch, candidates } => {
				let known = self.state().peers.contains_key(&peer);
				if known {
					self.accept_offer(peer, epoch, candidates);
				} else {
					self.state().early_offers.insert(peer, (epoch, candidates));
				}
			}
			MediaSignal::Ready {
				call,
				ssrc,
				generation,
				epoch,
			} => {
				let ready = RemoteReady { ssrc, generation };
				{
					let mut state = self.state();
					match state.calls.get_mut(&call) {
						Some(media) if media.peer == peer => media.remote = Some(ready),
						Some(_) => return,
						None => {
							state
								.early_ready
								.insert(call, peer, epoch, ready, Instant::now());
							return;
						}
					}
				}
				self.activate_later(call);
			}
		}
	}

	pub(super) fn accept_offer(&self, peer: PeerId, epoch: [u8; 16], candidates: Vec<SocketAddr>) {
		let calls: Vec<CallId> = {
			let mut state = self.state();
			let Some(media) = state.peers.get_mut(&peer) else {
				return;
			};
			if media.epoch != epoch {
				return;
			}
			media.remote_candidates = Some(candidates);
			state
				.calls
				.iter()
				.filter(|(_, c)| c.peer == peer)
				.map(|(id, _)| *id)
				.collect()
		};
		calls.into_iter().for_each(|call| self.activate_later(call));
	}

	pub(super) fn activate_later(&self, call: CallId) {
		let system = self.clone();
		self.inner
			.runtime
			.spawn(async move { system.try_activate(call).await });
	}

	/// Installs both streams and validates the path once everything the call
	/// needs has arrived. Runs at most once per call.
	pub(super) async fn try_activate(&self, call: CallId) {
		let plan = {
			let mut state = self.state();
			if let Some((epoch, ready)) = state.early_ready.take(&call, Instant::now()) {
				let own_epoch = state
					.calls
					.get(&call)
					.and_then(|c| state.peers.get(&c.peer))
					.map(|p| p.epoch);
				if let (Some(media), true) = (state.calls.get_mut(&call), own_epoch == Some(epoch))
				{
					media.remote.get_or_insert(ready);
				}
			}
			let Some(media) = state.calls.get(&call) else {
				return;
			};
			let (Phase::Prepared, Some(remote)) = (media.phase, media.remote) else {
				return;
			};
			let Some(peer) = state.peers.get(&media.peer) else {
				return;
			};
			let Some(candidates) = peer.remote_candidates.clone() else {
				return;
			};
			let endpoint = Arc::clone(&peer.socket.endpoint);
			let source = peer.socket.source;
			let session = media.session;
			let media = state.calls.get_mut(&call).expect("checked above");
			media.phase = Phase::Activating;
			(
				remote,
				candidates,
				endpoint,
				source,
				session,
				media.sender.take(),
			)
		};
		let (remote, candidates, endpoint, source, session, sender) = plan;
		let Some(sender) = sender else {
			return;
		};
		if !endpoint.bind_stream(call.to_bytes(), remote.generation) {
			tracing::warn!(%call, "stream binding refused");
			return;
		}
		let receiver = match CallReceiver::new(
			RemoteStream {
				call,
				ssrc: remote.ssrc,
				generation: remote.generation,
				source,
			},
			self.inner.config.jitter,
		) {
			Ok(receiver) => receiver,
			Err(error) => {
				tracing::warn!(%error, %call, "receiver creation failed");
				return;
			}
		};
		if let Some(media) = self.state().calls.get_mut(&call) {
			media.recv_stats = Some(receiver.stats());
		}
		let installed = self
			.inner
			.render
			.push(RenderCmd::AddCall(Box::new(receiver)))
			.is_ok() && self
			.inner
			.capture
			.push(CaptureCmd::AddSender(sender))
			.is_ok();
		if !installed {
			tracing::warn!(%call, "worker queue full; call not activated");
			return;
		}
		for candidate in candidates {
			if endpoint.validate_path(candidate).await.is_ok() {
				if let Some(media) = self.state().calls.get_mut(&call) {
					media.phase = Phase::Active;
				}
				self.emit(MediaEvent::Peer(PeerEvent::PathValidated { call, session }));
				return;
			}
		}
		tracing::warn!(%call, "no media path could be validated");
	}

	/// Retires a call's media and reports `MediaStopped`. Idempotent.
	pub fn stop(&self, call: CallId, session: SessionId) {
		self.retire(call);
		let system = self.clone();
		self.inner.runtime.spawn(async move {
			system.emit(MediaEvent::System(SystemEvent::MediaStopped {
				call,
				session,
			}));
		});
	}

	pub(super) fn retire(&self, call: CallId) {
		let (removed_peer, none_left) = {
			let mut state = self.state();
			state.early_ready.discard(&call);
			let Some(media) = state.calls.remove(&call) else {
				return;
			};
			let peer_idle = !state.calls.values().any(|c| c.peer == media.peer);
			let peer = if peer_idle {
				state.peers.remove(&media.peer)
			} else {
				None
			};
			if let Some(peer) = &peer {
				peer.socket.endpoint.unbind_call(&call.to_bytes());
			} else if let Some(peer) = state.peers.get(&media.peer) {
				peer.socket.endpoint.unbind_call(&call.to_bytes());
			}
			let none_left = state.calls.is_empty() && !self.inner.mic_test.wants_playback();
			if none_left {
				state.playback_open = false;
			}
			(peer, none_left)
		};
		self.inner.board.remove(call);
		let _ = self.inner.capture.push(CaptureCmd::RemoveSender(call));
		let _ = self.inner.render.push(RenderCmd::RemoveCall(call));
		if let Some(peer) = removed_peer {
			let _ = self
				.inner
				.render
				.push(RenderCmd::RemoveSource(peer.socket.source));
		}
		if none_left {
			let _ = self.inner.devices.commands.send(DeviceCmd::ClosePlayback);
		}
	}
}
