//! The single serialized structure kept in the OS credential store.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

use crate::error::{ConfigError, Result};

/// Layout version of the stored blob. A different version is refused, never guessed at.
pub(super) const BUNDLE_VERSION: u32 = 1;

/// Secret bytes, zeroed on drop, hex in the serialized form, never printed.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct SecretBytes(Zeroizing<Vec<u8>>);

impl SecretBytes {
	pub(super) fn new(bytes: &[u8]) -> Self {
		Self(Zeroizing::new(bytes.to_vec()))
	}

	pub(super) fn bytes(&self) -> &[u8] {
		&self.0
	}
}

impl fmt::Debug for SecretBytes {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("SecretBytes(<redacted>)")
	}
}

impl Serialize for SecretBytes {
	fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
		let text = Zeroizing::new(hex::encode(&*self.0));
		s.serialize_str(&text)
	}
}

impl<'de> Deserialize<'de> for SecretBytes {
	fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
		struct HexVisitor;
		impl Visitor<'_> for HexVisitor {
			type Value = SecretBytes;

			fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				f.write_str("a hex string")
			}

			fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<SecretBytes, E> {
				let bytes = hex::decode(v).map_err(|_| E::custom("secret is not valid hex"))?;
				Ok(SecretBytes(Zeroizing::new(bytes)))
			}
		}
		d.deserialize_str(HexVisitor)
	}
}

/// Everything Relay keeps in the credential store, under one service and account.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Bundle {
	version: u32,
	#[serde(default)]
	pub(super) identity: Option<SecretBytes>,
	/// `<channel hex>/<slot>` to secret.
	#[serde(default)]
	pub(super) channels: BTreeMap<String, SecretBytes>,
}

impl Default for Bundle {
	fn default() -> Self {
		Self {
			version: BUNDLE_VERSION,
			identity: None,
			channels: BTreeMap::new(),
		}
	}
}

impl fmt::Debug for Bundle {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Bundle")
			.field("version", &self.version)
			.field("identity", &self.identity.as_ref().map(|_| "<redacted>"))
			.field("channels", &self.channels.len())
			.finish()
	}
}

impl Bundle {
	pub(super) fn is_empty(&self) -> bool {
		self.identity.is_none() && self.channels.is_empty()
	}

	pub(super) fn encode(&self) -> Result<Zeroizing<Vec<u8>>> {
		serde_json::to_vec(self)
			.map(Zeroizing::new)
			.map_err(|e| ConfigError::Serialize {
				what: "credential bundle",
				message: e.to_string(),
			})
	}

	pub(super) fn decode(bytes: &[u8]) -> Result<Self> {
		let bundle: Self = serde_json::from_slice(bytes).map_err(|e| {
			ConfigError::BundleUnreadable(format!("malformed: {e}"))
		})?;
		if bundle.version != BUNDLE_VERSION {
			return Err(ConfigError::BundleUnreadable(format!(
				"version {} is not supported (expected {BUNDLE_VERSION})",
				bundle.version
			)));
		}
		Ok(bundle)
	}
}
