//! The subnet of the local interface a link arrived on, for media candidate filtering.

use std::net::IpAddr;

use ipnet::IpNet;

/// An address range (network and prefix length).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subnet(IpNet);

impl Subnet {
	/// The range of `ip` under a prefix of `prefix` bits (clamped to the family's width).
	pub fn new(ip: IpAddr, prefix: u8) -> Self {
		let width = if ip.is_ipv4() { 32 } else { 128 };
		Self(IpNet::new(ip, prefix.min(width)).unwrap_or_else(|_| ip.into()))
	}

	/// True if `other` is the same family and inside the range.
	pub fn contains(&self, other: IpAddr) -> bool {
		self.0.contains(&other)
	}

	/// The subnet of the interface that owns `local`, if the system lists it.
	pub fn of_interface(local: IpAddr) -> Option<Self> {
		if_addrs::get_if_addrs()
			.ok()?
			.into_iter()
			.find(|i| i.ip() == local)
			.and_then(|i| match i.addr {
				if_addrs::IfAddr::V4(v4) => {
					IpNet::with_netmask(v4.ip.into(), v4.netmask.into()).ok()
				}
				if_addrs::IfAddr::V6(v6) => {
					IpNet::with_netmask(v6.ip.into(), v6.netmask.into()).ok()
				}
			})
			.map(Self)
	}
}
