//! Inbound scope: a connection or datagram is admitted only if it arrived on an interface the
//! [`InterfacePolicy`] considers eligible (physical Wi-Fi or Ethernet, not a tunnel, VPN,
//! container or cellular interface) and its source lies inside that interface's own subnet.
//! Address class alone ("some private range") would admit every routed private network
//! behind a VPN, a campus router or a carrier.
//!
//! Overlay space (100.64.0.0/10) is not decided here: the [`AddressPolicy`] admits it only for
//! the one authenticated peer the user opted into, and that decision is kept as it is.

use std::net::{IpAddr, SocketAddr};

use crate::address::{AddressError, AddressPolicy, canonical, is_shared_address_space};
use crate::discovery::InterfacePolicy;

/// One address of one local interface, reduced to what the scope check needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iface {
	pub name: String,
	pub ip: IpAddr,
	pub prefix: u8,
	pub index: Option<u32>,
	pub loopback: bool,
}

/// The addresses of this device's interfaces right now (empty if the platform refuses).
pub fn interfaces() -> Vec<Iface> {
	let Ok(list) = if_addrs::get_if_addrs() else {
		return Vec::new();
	};
	list.into_iter()
		.map(|i| {
			let prefix = match &i.addr {
				if_addrs::IfAddr::V4(v4) => v4.prefixlen,
				if_addrs::IfAddr::V6(v6) => v6.prefixlen,
			};
			Iface {
				loopback: i.is_loopback(),
				ip: canonical(i.ip()),
				name: i.name,
				prefix,
				index: i.index,
			}
		})
		.collect()
}

/// True if `a` and `b` are the same family and agree on the first `prefix` bits.
pub(crate) fn same_prefix(a: IpAddr, b: IpAddr, prefix: u8) -> bool {
	let (width, a, b) = match (a, b) {
		(IpAddr::V4(a), IpAddr::V4(b)) => {
			(32u32, u128::from(u32::from(a)), u128::from(u32::from(b)))
		}
		(IpAddr::V6(a), IpAddr::V6(b)) => (128, u128::from(a), u128::from(b)),
		_ => return false,
	};
	let prefix = u32::from(prefix).min(width);
	if prefix == 0 {
		return true;
	}
	let mask = (u128::MAX >> (128 - width)) & !((1u128 << (width - prefix)) - 1);
	a & mask == b & mask
}

/// Which sources may reach the pre-authentication surface (control accept loop, transfer
/// hub, media path challenge). The default is the strict production behaviour.
#[derive(Debug, Clone, Default)]
pub struct InboundScope {
	interfaces: InterfacePolicy,
}

impl InboundScope {
	/// Scope using the interface rules the user configured for discovery.
	pub fn new(interfaces: InterfacePolicy) -> Self {
		Self { interfaces }
	}

	/// An accepted TCP connection: `local` is `stream.local_addr()`, `peer` its source.
	pub fn admit_tcp(
		&self,
		policy: &AddressPolicy,
		local: SocketAddr,
		peer: SocketAddr,
	) -> Result<(), AddressError> {
		self.admit_tcp_in(&interfaces(), policy, local, peer)
	}

	/// A UDP datagram source. The socket is bound to the wildcard, so the receiving
	/// interface is not known: any eligible interface whose subnet holds the source admits it.
	pub fn admit_udp(&self, policy: &AddressPolicy, peer: SocketAddr) -> Result<(), AddressError> {
		self.admit_udp_in(&interfaces(), policy, peer)
	}

	/// Whether `ip` (with the scope id of an IPv6 link-local one) is on-link with an eligible
	/// interface, optionally only those whose index is in `via`. Used for discovery records.
	pub fn is_on_link(&self, ip: IpAddr, scope: u32, via: &[u32]) -> bool {
		self.on_link_in(&interfaces(), ip, scope, via)
	}

	/// [`Self::admit_tcp`] against an explicit interface list.
	pub fn admit_tcp_in(
		&self,
		ifaces: &[Iface],
		policy: &AddressPolicy,
		local: SocketAddr,
		peer: SocketAddr,
	) -> Result<(), AddressError> {
		policy.check_socket(peer)?;
		let ip = canonical(peer.ip());
		if is_shared_address_space(ip) {
			return Ok(());
		}
		let local_ip = canonical(local.ip());
		if ip.is_loopback() {
			return if local_ip.is_loopback() {
				Ok(())
			} else {
				Err(AddressError::OffLink)
			};
		}
		let scope = scope_of(peer);
		let ok = ifaces.iter().any(|i| {
			i.ip == local_ip
				&& !i.loopback
				&& self.interfaces.admits_name(&i.name)
				&& contains(i, ip, scope, local_scope(local))
		});
		ok.then_some(()).ok_or(AddressError::OffLink)
	}

	/// [`Self::admit_udp`] against an explicit interface list.
	pub fn admit_udp_in(
		&self,
		ifaces: &[Iface],
		policy: &AddressPolicy,
		peer: SocketAddr,
	) -> Result<(), AddressError> {
		policy.check_socket(peer)?;
		let ip = canonical(peer.ip());
		if is_shared_address_space(ip) || ip.is_loopback() {
			return Ok(());
		}
		self.on_link_in(ifaces, ip, scope_of(peer), &[])
			.then_some(())
			.ok_or(AddressError::OffLink)
	}

	/// [`Self::is_on_link`] against an explicit interface list.
	pub fn on_link_in(&self, ifaces: &[Iface], ip: IpAddr, scope: u32, via: &[u32]) -> bool {
		let ip = canonical(ip);
		ifaces.iter().any(|i| {
			!i.loopback
				&& self.interfaces.admits_name(&i.name)
				&& (via.is_empty() || i.index.is_some_and(|n| via.contains(&n)))
				&& contains(i, ip, scope, None)
		})
	}
}

fn scope_of(addr: SocketAddr) -> u32 {
	match addr {
		SocketAddr::V6(v6) => v6.scope_id(),
		SocketAddr::V4(_) => 0,
	}
}

fn local_scope(addr: SocketAddr) -> Option<u32> {
	Some(scope_of(addr)).filter(|s| *s != 0)
}

/// `ip` is inside the interface's subnet; a link-local IPv6 source also needs the interface's
/// own scope (its index, or the scope of the local socket when the index is unknown).
fn contains(iface: &Iface, ip: IpAddr, scope: u32, local_scope: Option<u32>) -> bool {
	if !same_prefix(iface.ip, ip, iface.prefix) {
		return false;
	}
	match ip {
		IpAddr::V6(v6) if v6.is_unicast_link_local() => {
			scope != 0 && iface.index.or(local_scope).is_none_or(|own| own == scope)
		}
		_ => true,
	}
}
