use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr, SocketAddrV6};

use mdns_sd::{ResolvedService, ScopedIp};

use super::DiscoveryError;
use super::names::sanitize_display_name;
use crate::address::AddressPolicy;
use crate::fixed_hex;
use crate::identity::Fingerprint;
use crate::noise::PROTOCOL_MAJOR;

const HINT_BYTES: usize = 10;
const INSTANCE_ID_BYTES: usize = 16;
/// TXT strings are limited to 255 bytes including `n=`.
const ADVERTISED_NAME_MAX_BYTES: usize = 200;
const CAPS_MAX_BYTES: usize = 64;
const RESERVED_KEYS: [&str; 5] = ["v", "n", "id", "fp", "caps"];
/// Extra TXT entries published, and raw TXT entries retained per discovered peer.
pub const EXTRA_TXT_MAX: usize = 8;
const TXT_KEY_MAX_BYTES: usize = 8;
const TXT_VALUE_MAX_BYTES: usize = 200;

/// Leading bytes of a peer's claimed fingerprint. Only a matching aid: the type is distinct
/// from [`Fingerprint`] so a hint can never be mistaken for an authenticated identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FingerprintHint([u8; HINT_BYTES]);

impl FingerprintHint {
	pub fn of(fingerprint: &Fingerprint) -> Self {
		let mut hint = [0u8; HINT_BYTES];
		hint.copy_from_slice(&fingerprint.as_bytes()[..HINT_BYTES]);
		Self(hint)
	}

	/// Whether an authenticated fingerprint is consistent with this untrusted hint.
	pub fn matches(&self, fingerprint: &Fingerprint) -> bool {
		fingerprint.as_bytes()[..HINT_BYTES] == self.0
	}

	fn to_hex(self) -> String {
		hex::encode(self.0)
	}

	fn from_hex(text: &str) -> Option<Self> {
		Some(Self(fixed_hex::decode::<HINT_BYTES>(text)?))
	}
}

/// How much identity the record carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdvertMode {
	/// Display name and fingerprint hint are published (the default).
	#[default]
	Open,
	/// Neither the display name nor the fingerprint hint is published, so a LAN observer
	/// learns neither who the device is nor which channels it is in. Use a rotating
	/// pseudonymous `instance_id` (`channel::tag::hidden_instance_id`) and re-advertise when
	/// it changes. The device remains visible as an address; this is not anonymity.
	Hidden,
}

/// What the local device publishes. No keys, tokens, paths, call state or avatar bytes.
#[derive(Debug, Clone)]
pub struct Advertisement {
	/// Random per-install discovery instance id; unrelated to the display name.
	pub instance_id: [u8; INSTANCE_ID_BYTES],
	pub display_name: String,
	pub port: u16,
	pub fingerprint: Fingerprint,
	/// Small capability summary, e.g. `opus-mono-48k-10ms-v1`.
	pub capabilities: String,
	pub mode: AdvertMode,
	/// Additional short TXT entries (e.g. blinded channel tags). Keys are 1 to 8 lowercase
	/// ASCII letters or digits and must not shadow the core keys; at most 8 entries.
	pub extra_txt: Vec<(String, String)>,
}

impl Advertisement {
	pub(super) fn instance_name(&self) -> String {
		format!("relay-{}", hex::encode(&self.instance_id[..6]))
	}

	pub fn txt(&self) -> Result<HashMap<String, String>, DiscoveryError> {
		if self.port == 0 {
			return Err(DiscoveryError::Advertisement("port must be non-zero"));
		}
		if self.capabilities.len() > CAPS_MAX_BYTES || !self.capabilities.is_ascii() {
			return Err(DiscoveryError::Advertisement(
				"capability summary must be short ASCII",
			));
		}
		if self.extra_txt.len() > EXTRA_TXT_MAX {
			return Err(DiscoveryError::Advertisement("too many extra TXT entries"));
		}
		let mut map = HashMap::from([
			("v".to_owned(), PROTOCOL_MAJOR.to_string()),
			("id".to_owned(), hex::encode(self.instance_id)),
			("caps".to_owned(), self.capabilities.clone()),
		]);
		if self.mode == AdvertMode::Open {
			map.insert(
				"n".to_owned(),
				truncate(
					&sanitize_display_name(&self.display_name),
					ADVERTISED_NAME_MAX_BYTES,
				),
			);
			map.insert(
				"fp".to_owned(),
				FingerprintHint::of(&self.fingerprint).to_hex(),
			);
		}
		for (key, value) in &self.extra_txt {
			let key_ok = (1..=TXT_KEY_MAX_BYTES).contains(&key.len())
				&& key
					.bytes()
					.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
				&& !RESERVED_KEYS.contains(&key.as_str());
			if !key_ok || value.len() > TXT_VALUE_MAX_BYTES || !value.is_ascii() {
				return Err(DiscoveryError::Advertisement("invalid extra TXT entry"));
			}
			if map.insert(key.clone(), value.clone()).is_some() {
				return Err(DiscoveryError::Advertisement("duplicate TXT key"));
			}
		}
		Ok(map)
	}
}

/// A resolved, policy-filtered, still untrusted candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
	pub fullname: String,
	pub instance_id: [u8; INSTANCE_ID_BYTES],
	/// Sanitized for display; duplicates are permitted, so show `hint` or address too.
	pub display_name: String,
	pub fingerprint_hint: Option<FingerprintHint>,
	pub capabilities: String,
	pub addresses: Vec<SocketAddr>,
	/// Raw TXT entries other than the core keys, bounded and ASCII-only: untrusted input for
	/// extensions such as channel tags. Never interpreted here.
	pub txt: BTreeMap<String, String>,
}

fn truncate(text: &str, max_bytes: usize) -> String {
	text[..text.floor_char_boundary(max_bytes)].to_owned()
}

/// Converts a resolved mDNS record into a candidate. Returns `None` for records that are
/// malformed, of another protocol major, or that have no LAN-eligible address.
pub fn parse_resolved(service: &ResolvedService, policy: &AddressPolicy) -> Option<DiscoveredPeer> {
	let addresses = service
		.addresses
		.iter()
		.filter_map(|scoped| match scoped {
			ScopedIp::V4(v4) => Some(SocketAddr::new(IpAddr::V4(*v4.addr()), service.port)),
			ScopedIp::V6(v6) => Some(SocketAddr::V6(SocketAddrV6::new(
				*v6.addr(),
				service.port,
				0,
				v6.scope_id().index,
			))),
			_ => None,
		})
		.collect();
	let txt: Vec<(String, String)> = service
		.txt_properties
		.iter()
		.map(|p| (p.key().to_owned(), p.val_str().to_owned()))
		.collect();
	from_record(&service.fullname, service.port, addresses, &txt, policy)
}

/// A service resolved by a browser other than `mdns-sd` (Android's `NsdManager`), as
/// plain values. It goes through the same checks as an mDNS record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformRecord {
	pub fullname: String,
	pub port: u16,
	pub addresses: Vec<IpAddr>,
	pub txt: Vec<(String, String)>,
}

/// Converts a platform-resolved record into a candidate; same rules as [`parse_resolved`].
pub fn parse_platform_record(
	record: &PlatformRecord,
	policy: &AddressPolicy,
) -> Option<DiscoveredPeer> {
	let addresses = record
		.addresses
		.iter()
		.map(|ip| SocketAddr::new(*ip, record.port))
		.collect();
	from_record(
		&record.fullname,
		record.port,
		addresses,
		&record.txt,
		policy,
	)
}

fn from_record(
	fullname: &str,
	port: u16,
	addresses: Vec<SocketAddr>,
	txt: &[(String, String)],
	policy: &AddressPolicy,
) -> Option<DiscoveredPeer> {
	let get = |key: &str| txt.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
	if get("v")? != PROTOCOL_MAJOR.to_string() || port == 0 {
		return None;
	}
	let instance_id = fixed_hex::decode::<INSTANCE_ID_BYTES>(get("id")?)?;
	let display_name = sanitize_display_name(get("n").unwrap_or_default());
	let fingerprint_hint = get("fp").and_then(FingerprintHint::from_hex);
	let capabilities = get("caps")
		.filter(|c| c.len() <= CAPS_MAX_BYTES && c.is_ascii())
		.unwrap_or_default()
		.to_owned();
	let mut addresses: Vec<SocketAddr> = addresses
		.into_iter()
		.filter(|addr| policy.check_socket(*addr).is_ok())
		.collect();
	if addresses.is_empty() {
		return None;
	}
	addresses.sort();
	let txt = txt
		.iter()
		.filter(|(k, _)| !RESERVED_KEYS.contains(&k.as_str()))
		.filter(|(k, _)| k.len() <= TXT_KEY_MAX_BYTES)
		.filter(|(_, v)| v.len() <= TXT_VALUE_MAX_BYTES && v.is_ascii())
		.map(|(k, v)| (k.clone(), v.clone()))
		.take(EXTRA_TXT_MAX)
		.collect();
	Some(DiscoveredPeer {
		fullname: fullname.to_owned(),
		instance_id,
		display_name,
		fingerprint_hint,
		capabilities,
		addresses,
		txt,
	})
}

#[cfg(test)]
mod platform_tests {
	use super::*;

	fn record(addr: &str) -> PlatformRecord {
		let txt = [
			("v", PROTOCOL_MAJOR.to_string()),
			("id", "00112233445566778899aabbccddeeff".to_owned()),
			("n", "Laptop".to_owned()),
			("ch", "abc".to_owned()),
		];
		PlatformRecord {
			fullname: "Laptop._praxis-relay._tcp.local.".to_owned(),
			port: 4747,
			addresses: vec![addr.parse().unwrap()],
			txt: txt.map(|(k, v)| (k.to_owned(), v)).to_vec(),
		}
	}

	#[test]
	fn lan_record_becomes_a_candidate_with_extra_txt() {
		let peer = parse_platform_record(&record("192.168.1.20"), &AddressPolicy::lan_only())
			.expect("candidate");
		assert_eq!(peer.display_name, "Laptop");
		assert_eq!(peer.addresses, vec!["192.168.1.20:4747".parse().unwrap()]);
		assert_eq!(peer.txt.get("ch").map(String::as_str), Some("abc"));
	}

	#[test]
	fn policy_and_protocol_checks_still_apply() {
		let policy = AddressPolicy::lan_only();
		assert!(parse_platform_record(&record("8.8.8.8"), &policy).is_none());
		let mut other = record("192.168.1.20");
		other.txt[0].1 = "999".to_owned();
		assert!(parse_platform_record(&other, &policy).is_none());
	}
}
