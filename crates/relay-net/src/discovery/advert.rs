use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr, SocketAddrV6};

use mdns_sd::{ResolvedService, ScopedIp};

use super::DiscoveryError;
use super::names::sanitize_display_name;
use crate::address::AddressPolicy;
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
		self.0.iter().map(|b| format!("{b:02x}")).collect()
	}

	fn from_hex(text: &str) -> Option<Self> {
		Some(Self(decode_hex::<HINT_BYTES>(text)?))
	}
}

fn decode_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
	if text.len() != N * 2 || !text.is_ascii() {
		return None;
	}
	let mut out = [0u8; N];
	for (i, byte) in out.iter_mut().enumerate() {
		*byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
	}
	Some(out)
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
		format!(
			"relay-{}",
			self.instance_id[..6]
				.iter()
				.map(|b| format!("{b:02x}"))
				.collect::<String>()
		)
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
			(
				"id".to_owned(),
				self.instance_id
					.iter()
					.map(|b| format!("{b:02x}"))
					.collect(),
			),
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
	let mut end = text.len().min(max_bytes);
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	text[..end].to_owned()
}

/// Converts a resolved mDNS record into a candidate. Returns `None` for records that are
/// malformed, of another protocol major, or that have no LAN-eligible address.
pub fn parse_resolved(service: &ResolvedService, policy: &AddressPolicy) -> Option<DiscoveredPeer> {
	if service.txt_properties.get_property_val_str("v")? != PROTOCOL_MAJOR.to_string()
		|| service.port == 0
	{
		return None;
	}
	let instance_id =
		decode_hex::<INSTANCE_ID_BYTES>(service.txt_properties.get_property_val_str("id")?)?;
	let display_name = sanitize_display_name(
		service
			.txt_properties
			.get_property_val_str("n")
			.unwrap_or_default(),
	);
	let fingerprint_hint = service
		.txt_properties
		.get_property_val_str("fp")
		.and_then(FingerprintHint::from_hex);
	let capabilities = service
		.txt_properties
		.get_property_val_str("caps")
		.filter(|c| c.len() <= CAPS_MAX_BYTES && c.is_ascii())
		.unwrap_or_default()
		.to_owned();

	let mut addresses: Vec<SocketAddr> = service
		.addresses
		.iter()
		.filter_map(|scoped| {
			let addr = match scoped {
				ScopedIp::V4(v4) => SocketAddr::new(IpAddr::V4(*v4.addr()), service.port),
				ScopedIp::V6(v6) => SocketAddr::V6(SocketAddrV6::new(
					*v6.addr(),
					service.port,
					0,
					v6.scope_id().index,
				)),
				_ => return None,
			};
			policy.check_socket(addr).ok().map(|()| addr)
		})
		.collect();
	if addresses.is_empty() {
		return None;
	}
	addresses.sort();
	let txt = service
		.txt_properties
		.iter()
		.filter(|p| !RESERVED_KEYS.contains(&p.key()))
		.filter(|p| p.key().len() <= TXT_KEY_MAX_BYTES)
		.filter_map(|p| {
			let value = p.val_str();
			(value.len() <= TXT_VALUE_MAX_BYTES && value.is_ascii())
				.then(|| (p.key().to_owned(), value.to_owned()))
		})
		.take(EXTRA_TXT_MAX)
		.collect();
	Some(DiscoveredPeer {
		fullname: service.fullname.clone(),
		instance_id,
		display_name,
		fingerprint_hint,
		capabilities,
		addresses,
		txt,
	})
}
