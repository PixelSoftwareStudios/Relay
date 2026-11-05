//! Service port implementation for the peer manager.

use relay_core::{PeerId, PeerMessage};
use relay_net::identity::TrustState;
use tokio::runtime::Handle;

use super::{LocalProfile, PeerHandle, PeerSnapshot};
use crate::service::ports::{
	LocalProfile as PortProfile, PeerControl, PeerInfo, PeerTrust, PortError, TrustRequest,
};

/// Adapts a [`PeerHandle`] to the service's [`PeerControl`] port.
pub struct PeerAdapter {
	handle: PeerHandle,
	runtime: Handle,
	policy: relay_net::AddressPolicy,
}

impl PeerAdapter {
	/// Wraps a handle; async work is spawned on `runtime`.
	pub fn new(handle: PeerHandle, runtime: Handle, policy: relay_net::AddressPolicy) -> Self {
		Self {
			handle,
			runtime,
			policy,
		}
	}
}

fn unavailable(error: super::PeerError) -> PortError {
	PortError::Unavailable(error.to_string())
}

fn info(snapshot: &PeerSnapshot) -> Option<PeerInfo> {
	Some(PeerInfo {
		id: snapshot.id?,
		display_name: snapshot.name.clone(),
		addresses: snapshot.addresses.iter().map(ToString::to_string).collect(),
		trust: match snapshot.trust {
			Some(TrustState::Verified) => PeerTrust::Verified,
			Some(TrustState::UnverifiedFirstUse) => PeerTrust::TofuUnverified,
			None => PeerTrust::Unknown,
		},
		reachable: snapshot.linked,
		avatar: snapshot.avatar,
	})
}

impl PeerControl for PeerAdapter {
	fn ensure_link(&self, peer: PeerId) -> Result<(), PortError> {
		self.handle.ensure_link(peer).map_err(unavailable)
	}

	fn send(&self, peer: PeerId, message: PeerMessage) -> Result<(), PortError> {
		self.handle.send(peer, message).map_err(unavailable)
	}

	/// Answers from known addresses only. An unknown address starts a dial
	/// in the background and reports `Unavailable`; retry once the handshake
	/// has finished.
	fn resolve_address(&self, address: &str) -> Result<PeerId, PortError> {
		let addr = self
			.policy
			.parse_manual(address)
			.map_err(|e| PortError::Invalid(e.to_string()))?;
		let known = self
			.handle
			.peers()
			.iter()
			.find(|p| p.id.is_some() && p.addresses.contains(&addr))
			.and_then(|p| p.id);
		if let Some(id) = known {
			return Ok(id);
		}
		let handle = self.handle.clone();
		self.runtime.spawn(async move {
			if let Ok(id) = handle.dial(addr, None).await {
				let _ = handle.add_address(id, addr);
			}
		});
		Err(PortError::Unavailable("dialling; retry shortly".to_owned()))
	}

	fn peers(&self) -> Vec<PeerInfo> {
		self.handle.peers().iter().filter_map(info).collect()
	}

	fn decide_trust(&self, request: TrustRequest) -> Result<(), PortError> {
		if request.expected_key != request.peer {
			return Err(PortError::Invalid("key does not match the peer".to_owned()));
		}
		let state = match request.requested {
			PeerTrust::TofuUnverified => TrustState::UnverifiedFirstUse,
			PeerTrust::Verified if request.out_of_band_verified => TrustState::Verified,
			PeerTrust::Verified => {
				return Err(PortError::Invalid(
					"verified trust needs an out-of-band comparison".to_owned(),
				));
			}
			_ => return Err(PortError::Unsupported("that trust label")),
		};
		let (handle, peer) = (self.handle.clone(), request.peer);
		self.runtime.spawn(async move {
			if let Err(error) = handle.pin(peer, state).await {
				tracing::warn!(%error, "trust decision failed");
			}
		});
		Ok(())
	}

	fn profile_changed(&self, profile: &PortProfile) {
		let _ = self.handle.set_profile(LocalProfile {
			display_name: profile.display_name.clone(),
			avatar: profile.avatar,
		});
	}
}
