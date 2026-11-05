//! Display profile exchange: name and avatar hash over the authenticated
//! link, avatar bytes by request.
//!
//! Everything here is display-only. A name or avatar never confers trust (the
//! fingerprint is the identity), nothing is requested from discovery hints,
//! and every size and hash the peer states is treated as a claim: sizes only
//! cap what we are willing to buffer, and the final bytes are hashed and
//! validated locally before they enter the cache.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use relay_config::avatar::{AvatarCache, limits::ENCODED_MAX_BYTES};
use relay_net::control::new_envelope;
use relay_net::discovery::sanitize_display_name;
use relay_net::identity::Fingerprint;
use relay_net::proto::{AvatarChunk, AvatarRequest, Envelope, Profile, envelope::Body};

use super::handle::{PeerOutput, peer_id};

/// Largest avatar chunk (matches the envelope validation limit).
pub const CHUNK_BYTES: usize = 8192;
const REJECTED_MAX: usize = 32;

/// The local profile.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalProfile {
	/// Display name; sanitized before it leaves the process.
	pub display_name: String,
	/// Content hash of the local avatar, which must be in the avatar cache.
	pub avatar: Option<[u8; 32]>,
}

/// Work the manager must carry out for the profile logic.
pub(super) enum Action {
	Send(Fingerprint, Envelope),
	Output(PeerOutput),
	/// Read the local avatar and send it in bulk-priority chunks.
	Serve(Fingerprint, [u8; 32]),
	/// Hash, validate and cache received bytes off the actor thread.
	Validate(Fingerprint, [u8; 32], Vec<u8>),
}

struct Incoming {
	hash: [u8; 32],
	buf: Vec<u8>,
	limit: usize,
}

pub(super) struct Profiles {
	local: LocalProfile,
	local_size: u32,
	cache: Option<Arc<AvatarCache>>,
	incoming: HashMap<Fingerprint, Incoming>,
	serving: HashSet<Fingerprint>,
	rejected: VecDeque<[u8; 32]>,
}

impl Profiles {
	pub(super) fn new(local: LocalProfile, cache: Option<Arc<AvatarCache>>) -> Self {
		let mut profiles = Self {
			local: LocalProfile::default(),
			local_size: 0,
			cache,
			incoming: HashMap::new(),
			serving: HashSet::new(),
			rejected: VecDeque::new(),
		};
		profiles.set_local(local);
		profiles
	}

	pub(super) fn local(&self) -> &LocalProfile {
		&self.local
	}

	/// Replaces the local profile. An avatar missing from the cache is dropped
	/// rather than advertised, so we never announce bytes we cannot serve.
	pub(super) fn set_local(&mut self, mut profile: LocalProfile) {
		profile.display_name = sanitize_display_name(&profile.display_name);
		self.local_size = 0;
		if let Some(hash) = profile.avatar {
			let size = self
				.cache
				.as_ref()
				.and_then(|cache| cache.get(&hash).ok().flatten())
				.map(|bytes| bytes.len());
			match size {
				Some(len) if len <= ENCODED_MAX_BYTES => self.local_size = len as u32,
				_ => profile.avatar = None,
			}
		}
		self.local = profile;
	}

	pub(super) fn local_envelope(&self) -> Envelope {
		new_envelope(Body::Profile(Profile {
			display_name: self.local.display_name.clone(),
			avatar_sha256: self.local.avatar.map(|h| h.to_vec()).unwrap_or_default(),
			avatar_size: self.local_size,
		}))
	}

	pub(super) fn forget(&mut self, peer: &Fingerprint) {
		self.incoming.remove(peer);
		self.serving.remove(peer);
	}

	pub(super) fn finish_serving(&mut self, peer: &Fingerprint) {
		self.serving.remove(peer);
	}

	fn is_rejected(&self, hash: &[u8; 32]) -> bool {
		self.rejected.contains(hash)
	}

	fn reject(&mut self, hash: [u8; 32]) {
		self.rejected.push_back(hash);
		if self.rejected.len() > REJECTED_MAX {
			self.rejected.pop_front();
		}
	}

	pub(super) fn on_profile(&mut self, peer: Fingerprint, message: &Profile) -> Vec<Action> {
		let claimed: Option<[u8; 32]> = message.avatar_sha256.as_slice().try_into().ok();
		let size = message.avatar_size as usize;
		let avatar = claimed.filter(|_| size > 0 && size <= ENCODED_MAX_BYTES);
		let mut actions = vec![Action::Output(PeerOutput::Profile {
			peer: peer_id(&peer),
			name: sanitize_display_name(&message.display_name),
			avatar,
		})];
		let Some(hash) = avatar else {
			return actions;
		};
		let cached = self
			.cache
			.as_ref()
			.is_some_and(|cache| matches!(cache.get(&hash), Ok(Some(_))));
		if cached {
			actions.push(Action::Output(PeerOutput::AvatarReady {
				peer: peer_id(&peer),
				hash,
			}));
		} else if self.cache.is_some()
			&& !self.is_rejected(&hash)
			&& !self.incoming.contains_key(&peer)
		{
			self.incoming.insert(
				peer,
				Incoming {
					hash,
					buf: Vec::new(),
					limit: size,
				},
			);
			actions.push(Action::Send(
				peer,
				new_envelope(Body::AvatarRequest(AvatarRequest {
					avatar_sha256: hash.to_vec(),
				})),
			));
		}
		actions
	}

	/// Serves only our own advertised avatar, one transfer per peer.
	pub(super) fn on_request(&mut self, peer: Fingerprint, request: &AvatarRequest) -> Vec<Action> {
		let wanted: Option<[u8; 32]> = request.avatar_sha256.as_slice().try_into().ok();
		match (wanted, self.local.avatar) {
			(Some(wanted), Some(ours)) if wanted == ours && self.serving.insert(peer) => {
				vec![Action::Serve(peer, ours)]
			}
			_ => Vec::new(),
		}
	}

	pub(super) fn on_chunk(&mut self, peer: Fingerprint, chunk: &AvatarChunk) -> Vec<Action> {
		let Some(incoming) = self.incoming.get_mut(&peer) else {
			return Vec::new();
		};
		let in_order = chunk.offset as usize == incoming.buf.len();
		let for_request = chunk.avatar_sha256 == incoming.hash;
		let fits = incoming.buf.len() + chunk.data.len() <= incoming.limit;
		if !(in_order && for_request && fits) {
			return self.abort(peer, "unexpected avatar chunk");
		}
		incoming.buf.extend_from_slice(&chunk.data);
		if !chunk.final_chunk {
			return Vec::new();
		}
		let Some(done) = self.incoming.remove(&peer) else {
			return Vec::new();
		};
		vec![Action::Validate(peer, done.hash, done.buf)]
	}

	fn abort(&mut self, peer: Fingerprint, reason: &str) -> Vec<Action> {
		let Some(incoming) = self.incoming.remove(&peer) else {
			return Vec::new();
		};
		self.reject(incoming.hash);
		vec![Action::Output(PeerOutput::AvatarRejected {
			peer: peer_id(&peer),
			hash: incoming.hash,
			reason: reason.to_owned(),
		})]
	}

	/// Outcome of validating received bytes; `stored` is the hash of what was
	/// actually cached.
	pub(super) fn validated(
		&mut self,
		peer: Fingerprint,
		expected: [u8; 32],
		stored: Result<[u8; 32], String>,
	) -> PeerOutput {
		match stored {
			Ok(hash) if hash == expected => PeerOutput::AvatarReady {
				peer: peer_id(&peer),
				hash,
			},
			other => {
				self.reject(expected);
				PeerOutput::AvatarRejected {
					peer: peer_id(&peer),
					hash: expected,
					reason: other.err().unwrap_or_else(|| "hash mismatch".to_owned()),
				}
			}
		}
	}

	/// The cache, for the serving task.
	pub(super) fn cache(&self) -> Option<Arc<AvatarCache>> {
		self.cache.clone()
	}
}

/// Splits avatar bytes into bounded chunk envelopes.
pub(super) fn chunks(hash: [u8; 32], bytes: &[u8]) -> Vec<Envelope> {
	let total = bytes.len() as u32;
	let count = bytes.chunks(CHUNK_BYTES).count();
	bytes
		.chunks(CHUNK_BYTES)
		.enumerate()
		.map(|(i, data)| {
			new_envelope(Body::AvatarChunk(AvatarChunk {
				avatar_sha256: hash.to_vec(),
				offset: (i * CHUNK_BYTES) as u32,
				total_size: total,
				data: data.to_vec(),
				final_chunk: i + 1 == count,
			}))
		})
		.collect()
}
