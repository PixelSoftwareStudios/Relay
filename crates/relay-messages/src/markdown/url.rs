//! Strict http(s) URL recognition and host analysis.

use super::punycode;
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

#[derive(PartialEq, Eq, Clone, Copy)]
enum Script {
	Latin,
	Cyrillic,
	Greek,
	Han,
	Kana,
	Hangul,
	Arabic,
	Hebrew,
	Other,
}

fn script(c: char) -> Option<Script> {
	if c.is_ascii_digit() || c == '-' {
		return None;
	}
	Some(match c as u32 {
		0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x24F | 0x1E00..=0x1EFF => Script::Latin,
		0x370..=0x3FF | 0x1F00..=0x1FFF => Script::Greek,
		0x400..=0x52F => Script::Cyrillic,
		0x590..=0x5FF => Script::Hebrew,
		0x600..=0x6FF => Script::Arabic,
		0x3040..=0x30FF => Script::Kana,
		0x4E00..=0x9FFF => Script::Han,
		0xAC00..=0xD7AF => Script::Hangul,
		_ => Script::Other,
	})
}

fn mixed(label: &str) -> bool {
	let mut seen: Option<Script> = None;
	for s in label.chars().filter_map(script) {
		match seen {
			None => seen = Some(s),
			Some(p) if p != s => return true,
			_ => {}
		}
	}
	false
}

fn analyse_host(host: &str) -> Option<HostInfo> {
	if host.is_empty() || host.len() > 253 {
		return None;
	}
	let ascii = host.to_ascii_lowercase();
	let mut display = Vec::new();
	let mut warning = None;
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
		if let Some(rest) = label.strip_prefix("xn--") {
			match punycode::decode(rest) {
				Some(d)
					if !d.is_ascii()
						&& !d.chars().any(|c| {
							c.is_control()
								|| c.is_whitespace() || crate::sanitize::is_format_char(c)
						}) =>
				{
					if mixed(&d) {
						warning = Some(HostWarning::MixedScript);
					} else if warning.is_none() {
						warning = Some(HostWarning::Idn);
					}
					display.push(d);
				}
				_ => {
					warning = Some(HostWarning::InvalidPunycode);
					display.push(label.to_owned());
				}
			}
		} else {
			display.push(label.to_owned());
		}
	}
	Some(HostInfo {
		ascii,
		display: display.join("."),
		warning,
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
