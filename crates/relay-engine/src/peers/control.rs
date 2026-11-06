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
use crate::service::ConnectFailure;
use crate::service::ports::{
	ConnectionDetails, ForgottenPeer, LocalProfile as PortProfile, PeerControl, PeerInfo,
	PeerSelector, PeerTrust, PortError, ReplacedPeer, TrustRequest,
};
use relay_net::connect::ConnectionString;

/// How long a scanned string may take to answer before it counts as unreachable.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

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

/// How long a trust decision may take, including dialling the device again.
const TRUST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// The stored state a valid request asks for.
fn trust_state(request: &TrustRequest) -> Result<TrustState, PortError> {
	if request.expected_key != request.peer {
		return Err(PortError::Invalid("key does not match the peer".to_owned()));
	}
	match request.requested {
		PeerTrust::TofuUnverified => Ok(TrustState::UnverifiedFirstUse),
		PeerTrust::Verified if request.out_of_band_verified => Ok(TrustState::Verified),
		PeerTrust::Verified => Err(PortError::Invalid(
			"verified trust needs an out-of-band comparison".to_owned(),
		)),
		_ => Err(PortError::Unsupported("that trust label")),
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
		key_change: None,
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

	fn connect_pinned(&self, text: &str) -> Result<PeerId, ConnectFailure> {
		let target = ConnectionString::parse(text, &self.policy)
			.map_err(|e| ConnectFailure::InvalidString(e.to_string()))?;
		let peer = peer_id(&target.expected);
		let handle = self.handle.clone();
		let dial = async move {
			let found =
				tokio::time::timeout(CONNECT_TIMEOUT, handle.dial(target.addr, Some(peer))).await;
			match found {
				Ok(Ok(found)) if found == peer => {
					// The handshake proved the key; the scanned code is the out-of-band
					// check, so the peer is verified (the same pin as DecidePeerTrust).
					handle
						.pin(peer, TrustState::Verified)
						.await
						.map_err(|e| ConnectFailure::Unreachable(e.to_string()))?;
					Ok(peer)
				}
				Ok(Ok(_)) => Err(ConnectFailure::Mismatch),
				Ok(Err(error)) => {
					let reason = error.to_string();
					Err(if reason.contains("expected fingerprint") {
						ConnectFailure::Mismatch
					} else {
						ConnectFailure::Unreachable(reason)
					})
				}
				Err(_) => Err(ConnectFailure::Unreachable("timed out".to_owned())),
			}
		};
		self.runtime.block_on(dial)
	}

	fn peers(&self) -> Vec<PeerInfo> {
		let mut found: Vec<PeerInfo> = self
			.handle
			.peers()
			.iter()
			.filter_map(info)
			.map(|mut found| {
				found.key_change = self.handle.key_change(found.id).map(|c| c.previous);
				found
			})
			.collect();
		// Blocked devices are listed (for Settings) but are never reachable.
		let blocked = self.handle.blocked();
		found.retain(|p| !blocked.iter().any(|(id, _)| *id == p.id));
		found.extend(blocked.into_iter().map(|(id, name)| PeerInfo {
			display_name: name,
			trust: PeerTrust::Blocked,
			..PeerInfo::bare(id)
		}));
		found
	}

	fn replace_pinned_key(
		&self,
		peer: PeerId,
		fingerprint: &str,
		verified: bool,
		via: Option<&str>,
	) -> Result<ReplacedPeer, PortError> {
		let addresses = via
			.and_then(|text| ConnectionString::parse(text, &self.policy).ok())
			.map(|target| vec![target.addr])
			.unwrap_or_default();
		self.handle
			.replace_pinned_key(peer, fingerprint, verified, addresses)
	}

	fn key_change_for(&self, text: &str) -> Option<PeerId> {
		let target = ConnectionString::parse(text, &self.policy).ok()?;
		let peer = peer_id(&target.expected);
		self.handle.key_change(peer).map(|_| peer)
	}

	fn confirm_trust(&self, request: &TrustRequest) -> Result<(), PortError> {
		if request.requested == PeerTrust::Blocked {
			return Ok(());
		}
		let state = trust_state(request)?;
		let (handle, peer) = (self.handle.clone(), request.peer);
		let pin = async move {
			tokio::time::timeout(TRUST_TIMEOUT, handle.pin_reaching(peer, state)).await
		};
		match self.runtime.block_on(pin) {
			Ok(Ok(())) => Ok(()),
			Ok(Err(super::PeerError::KeyMismatch)) => Err(PortError::KeyMismatch),
			Ok(Err(error)) => {
				tracing::warn!(%error, "trust decision failed");
				Err(PortError::Unavailable(error.to_string()))
			}
			Err(_) => {
				tracing::warn!("trust decision timed out");
				Err(PortError::Unavailable("timed out".to_owned()))
			}
		}
	}

	/// The pin itself happened in [`Self::confirm_trust`]; this only re-checks the request.
	fn decide_trust(&self, request: TrustRequest) -> Result<(), PortError> {
		if request.requested == PeerTrust::Blocked {
			if request.expected_key != request.peer {
				return Err(PortError::Invalid("key does not match the peer".to_owned()));
			}
			return self.handle.block(request.peer);
		}
		trust_state(&request).map(drop)
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
