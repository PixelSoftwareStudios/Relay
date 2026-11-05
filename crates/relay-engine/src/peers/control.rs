//! Service port implementation for the peer manager.

use relay_core::{PeerId, PeerMessage};
use relay_net::connect::LocalAddress;
use relay_net::discovery::InterfacePolicy;
use relay_net::identity::TrustState;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::watch;

use super::handle::AdvertSpec;
use super::{LocalProfile, PeerHandle, PeerSnapshot, peer_id};
use crate::service::ports::{
	ConnectionDetails, ForgottenPeer, LocalProfile as PortProfile, PeerControl, PeerInfo, PeerSelector,
	PeerTrust, PortError, TrustRequest,
};
use relay_net::connect::ConnectionString;

/// Adapts a [`PeerHandle`] to the service's [`PeerControl`] port.
pub struct PeerAdapter {
	handle: PeerHandle,
	runtime: Handle,
	/// Policy for user-typed addresses (may allow overlay space).
	policy: relay_net::AddressPolicy,
	/// The plain LAN policy: what is advertised and shown to other people.
	lan: relay_net::AddressPolicy,
	interfaces: InterfaceSource,
	advert: Option<Arc<watch::Sender<AdvertSpec>>>,
}

/// Lists the addresses of this machine's network interfaces.
pub type InterfaceSource = Arc<dyn Fn() -> Vec<LocalAddress> + Send + Sync>;

/// The operating system's interface addresses.
pub fn system_interfaces() -> Vec<LocalAddress> {
	if_addrs::get_if_addrs()
		.unwrap_or_default()
		.into_iter()
		.map(|interface| LocalAddress {
			interface: interface.name.clone(),
			ip: interface.ip(),
		})
		.collect()
}

impl PeerAdapter {
	/// Wraps a handle; async work is spawned on `runtime`. `lan` is the plain
	/// policy for anything shown or advertised, `manual` the one for addresses
	/// the user types.
	pub fn new(
		handle: PeerHandle,
		runtime: Handle,
		lan: relay_net::AddressPolicy,
		manual: relay_net::AddressPolicy,
	) -> Self {
		Self {
			handle,
			runtime,
			policy: manual,
			lan,
			interfaces: Arc::new(system_interfaces),
			advert: None,
		}
	}

	/// Replaces the interface lister (tests).
	pub fn with_interfaces(mut self, interfaces: InterfaceSource) -> Self {
		self.interfaces = interfaces;
		self
	}

	/// Lets profile changes switch the discovery advertisement between visible
	/// and hidden.
	pub fn with_advert(mut self, advert: Arc<watch::Sender<AdvertSpec>>) -> Self {
		self.advert = Some(advert);
		self
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
		if address.trim_start().starts_with("relay://") {
			// A connection string names the peer's key, so no lookup is needed:
			// remember where to find it and let the link dial with that key pinned.
			let target = ConnectionString::parse(address, &self.policy)
				.map_err(|e| PortError::Invalid(e.to_string()))?;
			let peer = peer_id(&target.expected);
			self.handle
				.add_address(peer, target.addr)
				.map_err(unavailable)?;
			return Ok(peer);
		}
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

	fn forget_peer(&self, selector: &PeerSelector) -> Result<ForgottenPeer, PortError> {
		self.handle.forget(selector)
	}

	fn connection_info(&self) -> Result<ConnectionDetails, PortError> {
		let port = self.handle.local_addr().port();
		let fingerprint = self.handle.fingerprint();
		let found = relay_net::connect::encode(
			port,
			&fingerprint,
			&(self.interfaces)(),
			&self.lan,
			&InterfacePolicy::default(),
		);
		Ok(ConnectionDetails {
			listen_port: port,
			fingerprint: *fingerprint.as_bytes(),
			addresses: found.iter().map(|target| target.addr.ip()).collect(),
			connection_string: found.first().map(ToString::to_string),
		})
	}

	fn presence_changed(&self, attached: bool) {
		if let Some(advert) = &self.advert {
			advert.send_if_modified(|spec| {
				let changed = spec.present != attached;
				spec.present = attached;
				changed
			});
		}
	}

	fn profile_changed(&self, profile: &PortProfile) {
		if let Some(advert) = &self.advert {
			advert.send_if_modified(|spec| {
				let changed = spec.hidden != profile.hidden;
				spec.hidden = profile.hidden;
				changed
			});
		}
		let _ = self.handle.set_profile(LocalProfile {
			display_name: profile.display_name.clone(),
			avatar: profile.avatar,
		});
	}
}
