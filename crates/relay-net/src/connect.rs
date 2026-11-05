//! Connection strings: `relay://<ip>:<port>#<fingerprint>`.
//!
//! A string is an address plus the **expected device fingerprint**. Parsing never resolves
//! names and applies the LAN-only [`AddressPolicy`]; the fingerprint is mandatory, because a
//! string without one would be trust on first use hidden inside a link. The caller dials the
//! address and passes the fingerprint to the handshake, which refuses the peer before our
//! static key is revealed:
//!
//! ```ignore
//! let target = ConnectionString::parse(text, &AddressPolicy::lan_only())?;
//! let session = handshake::initiate(&mut stream, &identity, Some(&target.expected)).await?;
//! ```
//!
//! Pinning proves the peer holds the key whose fingerprint was in the string. It is only as
//! trustworthy as the channel the string travelled over (a pasted message, a QR code), and it
//! does not by itself mark the peer `Verified` in the trust store.
//!
//! Forms: `relay://192.168.1.20:7000#<64 hex>`, `relay://[fd12:3456::1]:7000#<64 hex>`.
//! IPv6 link-local needs a scope id that only means something on the device that holds the
//! interface, so [`encode`] never emits link-local IPv6; the parser still accepts
//! `[fe80::1%3]` for strings a user writes by hand on the same machine.

use std::fmt;
use std::net::{IpAddr, SocketAddr};

use thiserror::Error;

use crate::address::{AddressError, AddressPolicy};
use crate::discovery::InterfacePolicy;
use crate::identity::Fingerprint;

const SCHEME: &str = "relay://";
const INPUT_MAX_BYTES: usize = 256;
/// Addresses returned by [`encode`] at most.
pub const ENCODED_MAX: usize = 8;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConnectError {
	#[error("connection string is too long")]
	TooLong,
	#[error("connection string must start with relay://")]
	Scheme,
	#[error("connection string needs a #fingerprint of exactly 64 hex digits")]
	Fingerprint,
	#[error("connection string has credentials, a path or a query")]
	Extra,
	#[error(transparent)]
	Address(#[from] AddressError),
}

/// A parsed connection string: where to dial and whom to expect there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionString {
	pub addr: SocketAddr,
	pub expected: Fingerprint,
}

impl ConnectionString {
	pub fn new(addr: SocketAddr, expected: Fingerprint) -> Self {
		Self { addr, expected }
	}

	/// Parses and validates untrusted text. Surrounding whitespace is ignored.
	pub fn parse(text: &str, policy: &AddressPolicy) -> Result<Self, ConnectError> {
		let text = text.trim();
		if text.len() > INPUT_MAX_BYTES {
			return Err(ConnectError::TooLong);
		}
		let rest = text
			.get(..SCHEME.len())
			.filter(|p| p.eq_ignore_ascii_case(SCHEME))
			.map(|_| &text[SCHEME.len()..])
			.ok_or(ConnectError::Scheme)?;
		let (endpoint, fingerprint) = rest.split_once('#').ok_or(ConnectError::Fingerprint)?;
		if endpoint.contains(['@', '/', '?', '#', ' ', '\\']) {
			return Err(ConnectError::Extra);
		}
		let hex = fingerprint;
		if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
			return Err(ConnectError::Fingerprint);
		}
		let expected = Fingerprint::parse(hex).map_err(|_| ConnectError::Fingerprint)?;
		let addr = policy.parse_manual(endpoint)?;
		Ok(Self { addr, expected })
	}
}

impl fmt::Display for ConnectionString {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let hex: String = self
			.expected
			.as_bytes()
			.iter()
			.map(|b| format!("{b:02x}"))
			.collect();
		match self.addr {
			SocketAddr::V4(a) => write!(f, "{SCHEME}{a}#{hex}"),
			SocketAddr::V6(a) if a.scope_id() == 0 => {
				write!(f, "{SCHEME}[{}]:{}#{hex}", a.ip(), a.port())
			}
			SocketAddr::V6(a) => {
				write!(
					f,
					"{SCHEME}[{}%{}]:{}#{hex}",
					a.ip(),
					a.scope_id(),
					a.port()
				)
			}
		}
	}
}

/// A local address with the interface it belongs to, as reported by the platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalAddress {
	pub interface: String,
	pub ip: IpAddr,
}

/// Connection strings for this device, best first, at most [`ENCODED_MAX`].
///
/// Excluded: interfaces the [`InterfacePolicy`] refuses (utun, tun, tap, wg, tailscale, docker
/// and the like, unless named), anything the [`AddressPolicy`] refuses (public, loopback,
/// 100.64.0.0/10 overlay space) and link-local IPv6 (its scope id is not portable). Ordering:
/// private IPv4 on a physical-looking interface, then ULA IPv6, then IPv4 link-local (169.254,
/// which usually means no DHCP). Within a class the platform order is kept.
pub fn encode(
	port: u16,
	fingerprint: &Fingerprint,
	addresses: &[LocalAddress],
	policy: &AddressPolicy,
	interfaces: &InterfacePolicy,
) -> Vec<ConnectionString> {
	let mut ranked: Vec<(u8, SocketAddr)> = addresses
		.iter()
		.filter(|a| interfaces.admits_name(&a.interface))
		.filter_map(|a| {
			let addr = SocketAddr::new(a.ip, port);
			policy.check_socket(addr).ok()?;
			let class = match a.ip {
				IpAddr::V4(v4) if v4.is_private() => 0,
				IpAddr::V6(v6) if (v6.segments()[0] & 0xfe00) == 0xfc00 => 1,
				IpAddr::V4(v4) if v4.is_link_local() => 2,
				_ => return None,
			};
			Some((class, addr))
		})
		.collect();
	ranked.sort_by_key(|(class, _)| *class);
	ranked.dedup_by_key(|(_, addr)| *addr);
	ranked
		.into_iter()
		.take(ENCODED_MAX)
		.map(|(_, addr)| ConnectionString::new(addr, *fingerprint))
		.collect()
}
