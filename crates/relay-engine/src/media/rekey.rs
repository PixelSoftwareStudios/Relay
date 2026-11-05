//! Following the peer link's session: when a link is re-established (the 24 h
//! lifetime ended, or a fresh link replaced the old one) the media endpoint
//! switches to the new session instead of sealing with the dead one.

use std::sync::{Arc, Weak};
use std::time::Duration;

use relay_core::PeerId;
use relay_net::media::MediaEndpoint;

use super::system::{Inner, MediaSystem};

/// How often links are compared with the sessions media runs on.
const CHECK_EVERY: Duration = Duration::from_millis(500);

impl MediaSystem {
	/// The link epoch the peer's media currently runs on, for diagnostics.
	pub fn media_epoch(&self, peer: PeerId) -> Option<[u8; 16]> {
		self.state().peers.get(&peer).map(|p| p.epoch)
	}

	/// One pass: switch every peer whose link has a newer session.
	pub(super) async fn follow_links(&self) {
		let peers: Vec<(PeerId, [u8; 16], Arc<MediaEndpoint>)> = self
			.state()
			.peers
			.iter()
			.map(|(id, p)| (*id, p.epoch, Arc::clone(&p.socket.endpoint)))
			.collect();
		for (peer, epoch, endpoint) in peers {
			let Some(link) = self.inner.peers.link(peer).await else {
				continue;
			};
			if link.session.needs_rekey() {
				tracing::warn!(%peer, "link key lifetime is nearly spent; it will be replaced at the hard limit");
			}
			let next = *link.session.epoch().as_bytes();
			if next == epoch {
				continue;
			}
			let Some(path) = endpoint.validated_path() else {
				continue;
			};
			match endpoint.switch_epoch(Arc::clone(&link.session), path).await {
				Ok(()) => {
					if let Some(media) = self.state().peers.get_mut(&peer) {
						media.epoch = next;
					}
					tracing::info!(%peer, "media switched to the new link epoch");
				}
				// The remote may not have installed its side yet; try again.
				Err(error) => tracing::debug!(%peer, %error, "media epoch switch not complete"),
			}
		}
	}
}

/// Runs [`MediaSystem::follow_links`] until the system is dropped.
pub(super) async fn follow(inner: Weak<Inner>) {
	loop {
		tokio::time::sleep(CHECK_EVERY).await;
		let Some(inner) = inner.upgrade() else {
			return;
		};
		if inner.stop.load(std::sync::atomic::Ordering::Acquire) {
			return;
		}
		MediaSystem { inner }.follow_links().await;
	}
}
