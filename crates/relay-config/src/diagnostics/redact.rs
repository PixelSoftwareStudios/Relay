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
	} else if is_hex_run(core) || looks_like_path(core) || looks_like_ip(core) {
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
			return format!("{k}={}", redact_value(v));
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

/// Redacts one line: secrets after sensitive keys, long hex (keys, hashes, ids),
/// file paths and IP addresses. `Bearer <x>` style tokens are removed too.
pub fn redact_line(line: &str) -> String {
	let mut out: Vec<String> = Vec::new();
	let mut hide_next = false;
	for word in line.split_whitespace() {
		if hide_next {
			out.push("<redacted>".to_owned());
			hide_next = false;
			continue;
		}
		if word.eq_ignore_ascii_case("bearer") {
			hide_next = true;
		}
		out.push(redact_word(word));
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
