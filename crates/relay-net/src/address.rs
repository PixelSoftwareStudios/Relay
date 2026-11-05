//! LAN address policy. Only private, link-local and unique-local addresses are accepted;
//! public addresses are rejected rather than reasoned about by string prefix.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};

use thiserror::Error;

use crate::proto::Endpoint;

/// Reasons an address or endpoint is refused.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AddressError {
	#[error("address is not a private or link-local LAN address")]
	NotLan,
	#[error("loopback addresses are not permitted by this policy")]
	Loopback,
	#[error("malformed endpoint")]
	Malformed,
	#[error("port must be non-zero")]
	ZeroPort,
	#[error("link-local IPv6 requires a numeric scope id")]
	MissingScope,
	#[error("endpoint must be a bare ip:port, not a URL")]
	NotBareEndpoint,
}

/// Which addresses may be advertised, dialled or used as media destinations.
#[derive(Debug, Clone, Copy, Default)]
pub struct AddressPolicy {
	allow_loopback: bool,
}

impl AddressPolicy {
	/// Production policy: loopback is rejected.
	pub const fn lan_only() -> Self {
		Self {
			allow_loopback: false,
		}
	}

	/// Development and test policy that also admits loopback. Never used for advertisement.
	pub const fn lan_and_loopback() -> Self {
		Self {
			allow_loopback: true,
		}
	}

	/// Checks a single IP address against the policy.
	pub fn check_ip(&self, ip: IpAddr) -> Result<(), AddressError> {
		let ip = canonical(ip);
		if ip.is_loopback() {
			return if self.allow_loopback {
				Ok(())
			} else {
				Err(AddressError::Loopback)
			};
		}
		if is_lan_ip(ip) {
			Ok(())
		} else {
			Err(AddressError::NotLan)
		}
	}

	/// Checks a socket address: LAN address, non-zero port, scoped when link-local IPv6.
	pub fn check_socket(&self, addr: SocketAddr) -> Result<(), AddressError> {
		if addr.port() == 0 {
			return Err(AddressError::ZeroPort);
		}
		self.check_ip(addr.ip())?;
		if let SocketAddr::V6(v6) = addr
			&& is_v6_link_local(v6.ip())
			&& v6.scope_id() == 0
		{
			return Err(AddressError::MissingScope);
		}
		Ok(())
	}

	/// Parses a manually entered `ip:port`, `[v6]:port` or `[fe80::1%3]:port`.
	/// Host names, URLs and credentials are refused; no DNS lookup is ever performed.
	pub fn parse_manual(&self, input: &str) -> Result<SocketAddr, AddressError> {
		let input = input.trim();
		if input.contains("://") || input.contains(['@', '/', '?', '#', ' ']) {
			return Err(AddressError::NotBareEndpoint);
		}
		let addr = if let Some(rest) = input.strip_prefix('[') {
			let (host, port) = rest.split_once("]:").ok_or(AddressError::Malformed)?;
			let (ip, scope) = match host.split_once('%') {
				Some((ip, scope)) => (
					ip,
					scope.parse::<u32>().map_err(|_| AddressError::Malformed)?,
				),
				None => (host, 0),
			};
			let ip: Ipv6Addr = ip.parse().map_err(|_| AddressError::Malformed)?;
			let port: u16 = port.parse().map_err(|_| AddressError::Malformed)?;
			SocketAddr::V6(SocketAddrV6::new(ip, port, 0, scope))
		} else {
			let (ip, port) = input.rsplit_once(':').ok_or(AddressError::Malformed)?;
			let ip: Ipv4Addr = ip.parse().map_err(|_| AddressError::Malformed)?;
			let port: u16 = port.parse().map_err(|_| AddressError::Malformed)?;
			SocketAddr::new(IpAddr::V4(ip), port)
		};
		self.check_socket(addr)?;
		Ok(addr)
	}

	/// Converts a remote-offered candidate into a socket address. The remote scope index is
	/// local-interpretation data and is never trusted, so link-local candidates are refused
	/// here; they become usable only through a locally scoped, validated source address.
	pub fn candidate_from_proto(&self, endpoint: &Endpoint) -> Result<SocketAddr, AddressError> {
		let port = u16::try_from(endpoint.port).map_err(|_| AddressError::Malformed)?;
		let ip = match endpoint.ip_address.len() {
			4 => IpAddr::from(
				<[u8; 4]>::try_from(&endpoint.ip_address[..])
					.map_err(|_| AddressError::Malformed)?,
			),
			16 => IpAddr::from(
				<[u8; 16]>::try_from(&endpoint.ip_address[..])
					.map_err(|_| AddressError::Malformed)?,
			),
			_ => return Err(AddressError::Malformed),
		};
		if let IpAddr::V6(v6) = canonical(ip)
			&& is_v6_link_local(&v6)
		{
			return Err(AddressError::MissingScope);
		}
		let addr = SocketAddr::new(ip, port);
		self.check_socket(addr)?;
		Ok(addr)
	}
}

/// Builds a proto endpoint for a local socket address.
pub fn endpoint_to_proto(addr: SocketAddr) -> Endpoint {
	let ip_address = match addr.ip() {
		IpAddr::V4(v4) => v4.octets().to_vec(),
		IpAddr::V6(v6) => v6.octets().to_vec(),
	};
	let ipv6_scope_id = match addr {
		SocketAddr::V6(v6) => v6.scope_id(),
		SocketAddr::V4(_) => 0,
	};
	Endpoint {
		ip_address,
		port: u32::from(addr.port()),
		ipv6_scope_id,
	}
}

/// True for RFC 1918, IPv4 link-local, IPv6 link-local and IPv6 unique-local addresses.
pub fn is_lan_ip(ip: IpAddr) -> bool {
	match canonical(ip) {
		IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
		IpAddr::V6(v6) => is_v6_link_local(&v6) || (v6.segments()[0] & 0xfe00) == 0xfc00,
	}
}

fn is_v6_link_local(ip: &Ipv6Addr) -> bool {
	(ip.segments()[0] & 0xffc0) == 0xfe80
}

/// Unwraps IPv4-mapped IPv6 so a mapped public address cannot slip past the IPv4 checks.
fn canonical(ip: IpAddr) -> IpAddr {
	match ip {
		IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
		v4 => v4,
	}
}
