//! Strict http(s) URL recognition and host analysis.

use unicode_security::MixedScript;

use crate::limits::MD_MAX_URL_BYTES;

/// Why a host deserves a warning badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostWarning {
	/// Contains an `xn--` label (internationalised host).
	Idn,
	/// A label mixes scripts: the strong lookalike signal.
	MixedScript,
	/// An `xn--` label that does not decode.
	InvalidPunycode,
}

/// Host details for display. Clients show `ascii` whenever `warning` is set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostInfo {
	/// Lowercased ASCII host as it will be dialled.
	pub ascii: String,
	/// Decoded Unicode form (equals `ascii` when there is no IDN label).
	pub display: String,
	/// Warning, if any.
	pub warning: Option<HostWarning>,
}

/// An auto-detected link. Never opened automatically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
	/// The URL text exactly as it appeared.
	pub url: String,
	/// True for `https`.
	pub secure: bool,
	/// Host analysis.
	pub host: HostInfo,
}

fn analyse_host(host: &str) -> Option<HostInfo> {
	if host.is_empty() || host.len() > 253 {
		return None;
	}
	let ascii = host.to_ascii_lowercase();
	for label in ascii.split('.') {
		if label.is_empty()
			|| label.len() > 63
			|| label.starts_with('-')
			|| label.ends_with('-')
			|| !label
				.bytes()
				.all(|b| b.is_ascii_alphanumeric() || b == b'-')
		{
			return None;
		}
	}
	if !ascii.split('.').any(|l| l.starts_with("xn--")) {
		return Some(HostInfo {
			display: ascii.clone(),
			ascii,
			warning: None,
		});
	}
	// UTS #46 processing; labels that fail to decode are left as `xn--` text.
	let (display, result) = idna::domain_to_unicode(&ascii);
	let warning = if result.is_err() {
		HostWarning::InvalidPunycode
	} else if display.split('.').any(|l| !l.is_single_script()) {
		HostWarning::MixedScript
	} else {
		HostWarning::Idn
	};
	Some(HostInfo {
		ascii,
		display,
		warning: Some(warning),
	})
}

/// Parses `raw` as a linkifiable URL, or `None` when it must stay plain text.
pub fn parse_http(raw: &str) -> Option<Link> {
	if raw.len() > MD_MAX_URL_BYTES
		|| !raw.is_ascii()
		|| raw.bytes().any(|b| b <= b' ' || b == 0x7F)
	{
		return None;
	}
	let lower = raw.to_ascii_lowercase();
	let (secure, r) = match lower.strip_prefix("https://") {
		Some(r) => (true, r),
		None => (false, lower.strip_prefix("http://")?),
	};
	let rest = &raw[raw.len() - r.len()..];
	let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
	let authority = &rest[..end];
	if authority.contains('@') || authority.contains('[') {
		return None;
	}
	let host = match authority.rsplit_once(':') {
		Some((h, port)) => {
			if port.is_empty()
				|| port.len() > 5
				|| port.parse::<u32>().ok().is_none_or(|p| p > 65535)
			{
				return None;
			}
			h
		}
		None => authority,
	};
	let host = analyse_host(host)?;
	Some(Link {
		url: raw.to_owned(),
		secure,
		host,
	})
}
