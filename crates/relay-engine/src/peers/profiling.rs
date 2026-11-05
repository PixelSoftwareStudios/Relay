//! The manager's side of profile exchange: running [`Action`]s, serving
//! avatars at bulk priority and keeping the advertised name current.

use relay_net::identity::Fingerprint;
use relay_net::proto::envelope::Body;

use super::handle::{Command, PeerOutput};
use super::manager::Actor;
use super::profile::{Action, LocalProfile, chunks};

impl Actor {
	/// Sends our profile on a link that just became trusted.
	pub(super) async fn announce_profile(&mut self, peer: Fingerprint) {
		let envelope = self.profiles.local_envelope();
		self.send(peer, envelope).await;
	}

	/// Replaces the local profile: new mDNS advertisement and a `Profile`
	/// message on every trusted link.
	pub(super) async fn set_profile(&mut self, profile: LocalProfile) {
		self.profiles.set_local(profile);
		let applied = self.profiles.local().clone();
		self.params
			.display_name
			.store(applied.display_name.clone().into());
		let _ = self.profile_tx.send(applied);
		let peers: Vec<Fingerprint> = self
			.canonical
			.keys()
			.copied()
			.filter(|peer| self.trusted_link(peer).is_some())
			.collect();
		for peer in peers {
			self.announce_profile(peer).await;
		}
	}

	/// Handles a profile-related body from a trusted, canonical link.
	pub(super) async fn on_profile_body(&mut self, peer: Fingerprint, body: Body) {
		let actions = match body {
			Body::Profile(message) => self.profiles.on_profile(peer, &message),
			Body::AvatarRequest(request) => self.profiles.on_request(peer, &request),
			Body::AvatarChunk(chunk) => self.profiles.on_chunk(peer, &chunk),
			_ => Vec::new(),
		};
		self.run_actions(actions).await;
	}

	pub(super) async fn avatar_validated(
		&mut self,
		peer: Fingerprint,
		expected: [u8; 32],
		stored: Result<[u8; 32], String>,
	) {
		let output = self.profiles.validated(peer, expected, stored);
		self.emit(output).await;
	}

	async fn run_actions(&mut self, actions: Vec<Action>) {
		for action in actions {
			match action {
				Action::Send(peer, envelope) => self.send(peer, envelope).await,
				Action::Output(output) => {
					if let PeerOutput::Profile { peer, name, avatar } = &output {
						self.remote_profiles.insert(
							Fingerprint::from_bytes(*peer.as_bytes()),
							(name.clone(), *avatar),
						);
					}
					self.emit(output).await;
				}
				Action::Serve(peer, hash) => self.serve(peer, hash),
				Action::Validate(peer, expected, bytes) => self.validate(peer, expected, bytes),
			}
		}
	}

	fn validate(&self, peer: Fingerprint, expected: [u8; 32], bytes: Vec<u8>) {
		let Some(cache) = self.profiles.cache() else {
			return;
		};
		let commands = self.commands.clone();
		tokio::task::spawn_blocking(move || {
			let stored = cache.put_untrusted(&bytes).map_err(|e| e.to_string());
			let _ = commands.blocking_send(Command::AvatarValidated {
				peer,
				expected,
				stored,
			});
		});
	}

	/// Streams the local avatar through the link's bulk queue. Signaling and
	/// keepalives are taken first by the writer, so a transfer delays them
	/// by at most one chunk.
	fn serve(&mut self, peer: Fingerprint, hash: [u8; 32]) {
		let bulk = self
			.trusted_link(&peer)
			.map(|entry| entry.link.bulk.clone());
		let cache = self.profiles.cache();
		let commands = self.commands.clone();
		let (Some(bulk), Some(cache)) = (bulk, cache) else {
			self.profiles.finish_serving(&peer);
			return;
		};
		tokio::spawn(async move {
			let bytes = tokio::task::spawn_blocking(move || cache.get(&hash))
				.await
				.ok()
				.and_then(Result::ok)
				.flatten();
			if let Some(bytes) = bytes {
				for chunk in chunks(hash, &bytes) {
					if bulk.send(chunk).await.is_err() {
						break;
					}
				}
			}
			let _ = commands.send(Command::AvatarServed(peer)).await;
		});
	}
}
