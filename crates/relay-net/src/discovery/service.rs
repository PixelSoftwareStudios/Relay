use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use super::advert::{Advertisement, DiscoveredPeer, parse_resolved};
use super::interfaces::InterfacePolicy;
use super::{DiscoveryError, SERVICE_TYPE};
use crate::address::AddressPolicy;

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
				ServiceEvent::ServiceResolved(resolved) => {
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
