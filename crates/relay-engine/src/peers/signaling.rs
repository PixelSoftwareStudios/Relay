//! Mapping between relay-core peer messages and `relay.peer.v1` envelopes.
//!
//! The wire format carries no session identifier, so each engine keeps a
//! bounded call-to-session table: outbound invitations record the session the
//! domain chose, inbound ones derive a stable session from the call id.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;

use relay_core::{CallId, PeerEvent, PeerId, PeerMessage, SessionId};
use relay_net::AddressPolicy;
use relay_net::control::new_envelope;
use relay_net::proto::{
	Accept, End, Endpoint, EndpointOffer, Envelope, Invite, MediaReady, Reject, envelope::Body,
};

/// The single negotiated media profile.
pub const PROFILE: &str = "opus-48k-mono-10ms";
/// Ring timeout advertised in invitations (informational; the domain owns timers).
const RING_TIMEOUT_MS: u32 = 30_000;
const SESSIONS_MAX: usize = 256;
const REASON_BUSY: &str = "busy";
const REASON_DECLINED: &str = "declined";
const REASON_HANGUP: &str = "hangup";

/// Media negotiation traffic that belongs to the media module, not the domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaSignal {
	/// UDP candidates the sender listens on, bound to a link epoch.
	Offer {
		/// Epoch of the link the offer was sent on.
		epoch: [u8; 16],
		/// Candidate addresses that passed the LAN policy.
		candidates: Vec<SocketAddr>,
	},
	/// The sender's stream parameters for one call.
	Ready {
		/// The call.
		call: CallId,
		/// RTP SSRC the sender will use.
		ssrc: u32,
		/// Stream generation the sender will use.
		generation: u32,
		/// Epoch of the link the message was sent on.
		epoch: [u8; 16],
	},
}

/// A decoded inbound body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inbound {
	/// An event for the domain.
	Event(PeerEvent),
	/// Media negotiation.
	Media(MediaSignal),
	/// Valid but not acted on here (ringing, profile, advisory feedback).
	Ignored,
}

/// Call-to-session table, bounded with FIFO eviction.
#[derive(Debug, Default)]
pub struct SessionTable {
	map: HashMap<CallId, (SessionId, PeerId)>,
	order: VecDeque<CallId>,
}

impl SessionTable {
	fn remember(&mut self, call: CallId, session: SessionId, peer: PeerId) {
		if self.map.insert(call, (session, peer)).is_none() {
			self.order.push_back(call);
			if self.order.len() > SESSIONS_MAX
				&& let Some(oldest) = self.order.pop_front()
			{
				self.map.remove(&oldest);
			}
		}
	}

	/// The session recorded for a call.
	pub fn get(&self, call: CallId) -> Option<SessionId> {
		self.map.get(&call).map(|(session, _)| *session)
	}

	/// The peer a call was exchanged with.
	pub fn peer_of(&self, call: CallId) -> Option<PeerId> {
		self.map.get(&call).map(|(_, peer)| *peer)
	}
}

/// Session id for a call the remote started: stable and unpredictable to third parties.
pub fn derived_session(call: CallId) -> SessionId {
	let digest = blake3::hash(&call.to_bytes());
	let mut bytes = [0u8; 16];
	bytes.copy_from_slice(&digest.as_bytes()[..16]);
	SessionId::from_u128(u128::from_be_bytes(bytes))
}

fn call_bytes(call: CallId) -> Vec<u8> {
	call.to_bytes().to_vec()
}

fn parse_call(bytes: &[u8]) -> Option<CallId> {
	let raw: [u8; 16] = bytes.try_into().ok()?;
	Some(CallId::from_u128(u128::from_be_bytes(raw)))
}

/// Encodes a domain message, recording the session for outbound invitations.
pub fn encode(message: &PeerMessage, peer: PeerId, table: &mut SessionTable) -> Envelope {
	let body = match *message {
		PeerMessage::Invite {
			call,
			session,
			allow_parallel_same_peer,
		} => {
			table.remember(call, session, peer);
			Body::Invite(Invite {
				call_id: call_bytes(call),
				offered_profiles: vec![PROFILE.to_owned()],
				ring_timeout_ms: RING_TIMEOUT_MS,
				allow_parallel_same_peer,
			})
		}
		PeerMessage::Accept { call, session } => {
			table.remember(call, session, peer);
			Body::Accept(Accept {
				call_id: call_bytes(call),
				selected_profile: PROFILE.to_owned(),
				send_ssrc: 0,
				initial_stream_generation: 0,
			})
		}
		PeerMessage::Decline { call, busy, .. } => Body::Reject(Reject {
			call_id: call_bytes(call),
			reason_code: if busy { REASON_BUSY } else { REASON_DECLINED }.to_owned(),
		}),
		PeerMessage::End { call, .. } => Body::End(End {
			call_id: call_bytes(call),
			reason_code: REASON_HANGUP.to_owned(),
		}),
	};
	new_envelope(body)
}

/// Encodes media negotiation traffic.
pub fn encode_media(signal: &MediaSignal) -> Envelope {
	new_envelope(match signal {
		MediaSignal::Offer { epoch, candidates } => Body::EndpointOffer(EndpointOffer {
			link_epoch: epoch.to_vec(),
			candidates: candidates
				.iter()
				.map(|addr| relay_net::endpoint_to_proto(*addr))
				.collect(),
		}),
		MediaSignal::Ready {
			call,
			ssrc,
			generation,
			epoch,
		} => Body::MediaReady(MediaReady {
			call_id: call_bytes(*call),
			selected_profile: PROFILE.to_owned(),
			send_ssrc: *ssrc,
			send_stream_generation: *generation,
			link_epoch: epoch.to_vec(),
		}),
	})
}

/// Decodes an authenticated inbound body from `peer`.
///
/// Messages about a call this engine has no session for are ignored, except
/// an invitation, which creates the entry.
pub fn decode(
	peer: PeerId,
	body: Body,
	table: &mut SessionTable,
	policy: &AddressPolicy,
) -> Inbound {
	match body {
		Body::Invite(m) => {
			let Some(call) = parse_call(&m.call_id) else {
				return Inbound::Ignored;
			};
			let session = derived_session(call);
			table.remember(call, session, peer);
			Inbound::Event(PeerEvent::Invite {
				call,
				session,
				peer,
				allow_parallel_same_peer: m.allow_parallel_same_peer,
			})
		}
		Body::Accept(m) => with_session(&m.call_id, table, |call, session| PeerEvent::Accepted {
			call,
			session,
		}),
		Body::Reject(m) => with_session(&m.call_id, table, |call, session| PeerEvent::Declined {
			call,
			session,
		}),
		Body::End(m) => with_session(&m.call_id, table, |call, session| PeerEvent::Ended {
			call,
			session,
		}),
		Body::EndpointOffer(m) => {
			let Ok(epoch) = <[u8; 16]>::try_from(m.link_epoch.as_slice()) else {
				return Inbound::Ignored;
			};
			let candidates = m
				.candidates
				.iter()
				.filter_map(|e: &Endpoint| policy.candidate_from_proto(e).ok())
				.collect();
			Inbound::Media(MediaSignal::Offer { epoch, candidates })
		}
		Body::MediaReady(m) => {
			let (Some(call), Ok(epoch)) = (
				parse_call(&m.call_id),
				<[u8; 16]>::try_from(m.link_epoch.as_slice()),
			) else {
				return Inbound::Ignored;
			};
			Inbound::Media(MediaSignal::Ready {
				call,
				ssrc: m.send_ssrc,
				generation: m.send_stream_generation,
				epoch,
			})
		}
		_ => Inbound::Ignored,
	}
}

fn with_session(
	call_id: &[u8],
	table: &SessionTable,
	make: impl FnOnce(CallId, SessionId) -> PeerEvent,
) -> Inbound {
	parse_call(call_id)
		.and_then(|call| table.get(call).map(|session| make(call, session)))
		.map_or(Inbound::Ignored, Inbound::Event)
}
