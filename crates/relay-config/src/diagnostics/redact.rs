//! Text redaction for support bundles. Conservative: when unsure, redact.

use super::ip::looks_like_ip;

const SENSITIVE_KEYS: [&str; 8] = [
	"token",
	"secret",
	"key",
	"password",
	"passphrase",
	"auth",
	"credential",
	"seed",
];

fn is_hex_run(s: &str) -> bool {
	s.len() >= 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Keys whose value is a device or call identifier, however short.
const ID_KEYS: [&str; 6] = ["fingerprint", "fp", "peer", "device", "caller", "callee"];

fn is_id_key(key: &str) -> bool {
	let lower = key
		.trim_matches(|c: char| !c.is_ascii_alphanumeric())
		.to_ascii_lowercase();
	ID_KEYS.iter().any(|id| {
		lower == *id || lower.ends_with(&format!("_{id}")) || lower.ends_with(&format!("-{id}"))
	})
}

/// Hex digits, optionally grouped with `-` or `:` (`A1B2-C3D4`), at least 8 digits.
fn is_short_id(s: &str) -> bool {
	let core = s.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | '(' | ')' | '.'));
	let digits = core.bytes().filter(u8::is_ascii_hexdigit).count();
	digits >= 8
		&& core
			.bytes()
			.all(|b| b.is_ascii_hexdigit() || b == b'-' || b == b':')
}

/// `aa:bb:cc:dd:ee:ff` or `aa-bb-cc-dd-ee-ff`.
fn is_mac(s: &str) -> bool {
	let b = s.as_bytes();
	b.len() == 17
		&& b.iter().enumerate().all(|(i, c)| {
			if i % 3 == 2 {
				*c == b':' || *c == b'-'
			} else {
				c.is_ascii_hexdigit()
			}
		}) && b[2] == b[5]
		&& b[5] == b[8]
		&& b[8] == b[11]
		&& b[11] == b[14]
}

/// An mDNS host name such as `ann-laptop.local`, with an optional trailing dot or port.
fn is_local_host(s: &str) -> bool {
	let name = match s.rsplit_once(':') {
		Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
		_ => s,
	};
	let name = name.strip_suffix('.').unwrap_or(name);
	let lower = name.to_ascii_lowercase();
	lower.len() > ".local".len()
		&& lower.ends_with(".local")
		&& lower
			.bytes()
			.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn looks_like_path(s: &str) -> bool {
	let unix = s.starts_with('/') && s[1..].contains('/');
	let home = s.starts_with("~/");
	let b = s.as_bytes();
	let windows = b.len() > 3
		&& b[0].is_ascii_alphabetic()
		&& b[1] == b':'
		&& (b[2] == b'\\' || b[2] == b'/');
	unix || home || windows || s.starts_with("\\\\")
}

fn redact_value(value: &str) -> String {
	let quoted = value.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | '(' | ')'));
	let core = quoted.trim_matches(|c: char| matches!(c, '[' | ']'));
	// Brackets are part of an `[v6]:port` literal, so the IP test sees them.
	if looks_like_ip(quoted) {
		value.replace(quoted, "<redacted>")
	} else if is_hex_run(core)
		|| is_mac(core)
		|| is_local_host(core)
		|| looks_like_path(core)
		|| looks_like_ip(core)
	{
		value.replace(core, "<redacted>")
	} else {
		value.to_owned()
	}
}

fn redact_word(word: &str) -> String {
	if let Some((k, _)) = word.split_once(['=', ':']) {
		let lower = k.to_ascii_lowercase();
		if !k.is_empty() && SENSITIVE_KEYS.iter().any(|s| lower.contains(s)) {
			let sep = if word.contains('=') { '=' } else { ':' };
			return format!("{k}{sep}<redacted>");
		}
		if let Some((_, v)) = word.split_once('=') {
			if is_id_key(k) && is_short_id(v) {
				return format!("{k}=<redacted>");
			}
			return format!("{k}={}", redact_value(v));
		}
		if is_id_key(k)
			&& let Some((_, v)) = word.split_once(':')
			&& is_short_id(v)
		{
			return format!("{k}:<redacted>");
		}
	}
	let whole = redact_value(word);
	if whole == word
		&& let Some((k, v)) = word.split_once(':')
	{
		// `peer:fe80::1` style: the value follows the first colon.
		let tail = redact_value(v);
		if tail != v {
			return format!("{k}:{tail}");
		}
	}
	whole
}

/// Redacts one line: secrets after sensitive keys, long hex (keys, hashes, ids), short
/// hex after fingerprint-like keys, MAC addresses, `.local` host names, file paths and IP addresses. `Bearer <x>` style tokens are removed too.
pub fn redact_line(line: &str) -> String {
	let mut out: Vec<String> = Vec::new();
	let mut hide_next = false;
	let mut id_next = false;
	for word in line.split_whitespace() {
		if hide_next {
			out.push("<redacted>".to_owned());
			hide_next = false;
			continue;
		}
		if word.eq_ignore_ascii_case("bearer") {
			hide_next = true;
		}
		let redacted = redact_word(word);
		// `fingerprint A1B2-C3D4` and `peer 1a2b3c4d`: the id is the next word.
		if !word.contains(['=', ':']) && is_id_key(word) && !is_short_id(word) {
			id_next = true;
		} else if id_next && is_short_id(word) {
			id_next = false;
			out.push("<redacted>".to_owned());
			continue;
		} else {
			id_next = false;
		}
		out.push(redacted);
	}
	out.join(" ")
}

/// Short stable fingerprint so support can tell two device ids apart without seeing them.
pub fn fingerprint(value: &str) -> String {
	if value.is_empty() {
		return String::new();
	}
	format!("id:{}", &blake3::hash(value.as_bytes()).to_hex()[..8])
}

/// How the database location is shown in diagnostics: hidden unless `reveal` is
/// set, because the path contains the OS user name.
pub fn database_location(path: &std::path::Path, reveal: bool) -> String {
	if reveal {
		path.display().to_string()
	} else {
		"<redacted: per-user database>".to_owned()
	}
}
