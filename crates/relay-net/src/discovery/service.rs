use std::net::IpAddr;

use mdns_sd::{ScopedIp, ServiceDaemon, ServiceEvent, ServiceInfo};

use super::advert::{Advertisement, DiscoveredPeer, parse_resolved};
use super::interfaces::InterfacePolicy;
use super::{DiscoveryError, SERVICE_TYPE};
use crate::address::AddressPolicy;
use crate::onlink::InboundScope;

#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
	Found(Box<DiscoveredPeer>),
	Removed(String),
}

/// Owns the mDNS daemon (one background thread, counted in the resource budget).
pub struct Discovery {
	daemon: ServiceDaemon,
	events: mdns_sd::Receiver<ServiceEvent>,
	policy: AddressPolicy,
	scope: InboundScope,
	registered: Option<String>,
}

impl Discovery {
	/// Starts browsing on the eligible interfaces.
	pub fn start(
		interfaces: &InterfacePolicy,
		policy: AddressPolicy,
	) -> Result<Self, DiscoveryError> {
		let daemon = ServiceDaemon::new()?;
		interfaces.apply(&daemon)?;
		let events = daemon.browse(SERVICE_TYPE)?;
		Ok(Self {
			daemon,
			events,
			policy,
			scope: InboundScope::new(interfaces.clone()),
			registered: None,
		})
	}

	/// Publishes the local advertisement on the eligible interfaces.
	pub fn advertise(&mut self, advertisement: &Advertisement) -> Result<(), DiscoveryError> {
		let instance = advertisement.instance_name();
		let host = format!("{instance}.local.");
		let info = ServiceInfo::new(
			SERVICE_TYPE,
			&instance,
			&host,
			"",
			advertisement.port,
			advertisement.txt()?,
		)?
		.enable_addr_auto();
		let fullname = info.get_fullname().to_owned();
		self.daemon.register(info)?;
		self.registered = Some(fullname);
		Ok(())
	}

	/// Withdraws the advertisement, e.g. when the network changes.
	pub fn withdraw(&mut self) -> Result<(), DiscoveryError> {
		if let Some(name) = self.registered.take() {
			self.daemon.unregister(&name)?;
		}
		Ok(())
	}

	/// Next relevant event. Records that fail the address or format policy are skipped.
	pub async fn next_event(&self) -> Option<DiscoveryEvent> {
		loop {
			match self.events.recv_async().await.ok()? {
				ServiceEvent::ServiceResolved(mut resolved) => {
					retain_on_link(&mut resolved, &self.scope);
					if let Some(peer) = parse_resolved(&resolved, &self.policy) {
						return Some(DiscoveryEvent::Found(Box::new(peer)));
					}
				}
				ServiceEvent::ServiceRemoved(_, fullname) => {
					return Some(DiscoveryEvent::Removed(fullname));
				}
				_ => {}
			}
		}
	}

	pub fn shutdown(self) {
		let _ = self.daemon.shutdown();
	}
}

/// Drops advertised addresses that are not on-link with the interface the answer arrived on
/// (or, if the library did not record one, with any eligible interface). `mdns-sd` does not
/// check the datagram's source, so a host that can reach UDP 5353 from off-link could
/// otherwise name any private address; this keeps only addresses that could be a neighbour.
fn retain_on_link(resolved: &mut mdns_sd::ResolvedService, scope: &InboundScope) {
	resolved.addresses.retain(|scoped| {
		let (ip, scope_id, via): (IpAddr, u32, Vec<u32>) = match scoped {
			ScopedIp::V4(v4) => (
				IpAddr::V4(*v4.addr()),
				0,
				v4.interface_ids().iter().map(|i| i.index).collect(),
			),
			ScopedIp::V6(v6) => (
				IpAddr::V6(*v6.addr()),
				v6.scope_id().index,
				Some(v6.scope_id().index)
					.filter(|n| *n != 0)
					.into_iter()
					.collect(),
			),
			_ => return false,
		};
		scope.is_on_link(ip, scope_id, &via)
	});
}
