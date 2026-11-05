//! The subnet of the local interface a link arrived on, for media candidate filtering.

use std::net::IpAddr;

/// An address range (network and prefix length).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subnet {
	network: IpAddr,
	prefix: u8,
}

impl Subnet {
	/// The range of `ip` under a prefix of `prefix` bits.
	pub fn new(ip: IpAddr, prefix: u8) -> Self {
		Self {
			network: ip,
			prefix,
		}
	}

	/// True if `other` is the same family and inside the range.
	pub fn contains(&self, other: IpAddr) -> bool {
		match (self.network, other) {
			(IpAddr::V4(a), IpAddr::V4(b)) => {
				same_prefix(&a.octets(), &b.octets(), self.prefix.min(32))
			}
			(IpAddr::V6(a), IpAddr::V6(b)) => {
				same_prefix(&a.octets(), &b.octets(), self.prefix.min(128))
			}
			_ => false,
		}
	}

	/// The subnet of the interface that owns `local`, if the system lists it.
	pub fn of_interface(local: IpAddr) -> Option<Self> {
		if_addrs::get_if_addrs()
			.ok()?
			.into_iter()
			.find(|i| i.ip() == local)
			.and_then(|i| match i.addr {
				if_addrs::IfAddr::V4(v4) => Some(Self::new(
					v4.ip.into(),
					u32::from(v4.netmask).leading_ones().try_into().ok()?,
				)),
				if_addrs::IfAddr::V6(v6) => Some(Self::new(
					v6.ip.into(),
					u128::from(v6.netmask).leading_ones().try_into().ok()?,
				)),
			})
	}
}

fn same_prefix(a: &[u8], b: &[u8], bits: u8) -> bool {
	let (whole, rest) = (usize::from(bits / 8), bits % 8);
	if a[..whole] != b[..whole] {
		return false;
	}
	rest == 0 || (a[whole] ^ b[whole]) >> (8 - rest) == 0
}
