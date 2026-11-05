//! Text redaction for support bundles. Conservative: when unsure, redact.

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

fn looks_like_ip(s: &str) -> bool {
	let host = s.split(':').next().unwrap_or(s);
	let parts: Vec<&str> = host.split('.').collect();
	(parts.len() == 4
		&& parts
			.iter()
			.all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit())))
		|| s.matches(':').count() >= 3 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b':')
}

fn redact_value(value: &str) -> String {
	let core =
		value.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']'));
	if is_hex_run(core) || looks_like_path(core) || looks_like_ip(core) {
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
	redact_value(word)
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
