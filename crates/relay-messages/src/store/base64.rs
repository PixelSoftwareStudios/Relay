//! Minimal standard base64 (RFC 4648, padded) for export.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes `data`.
pub fn encode(data: &[u8]) -> String {
	let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
	for c in data.chunks(3) {
		let n = (u32::from(c[0]) << 16)
			| (u32::from(*c.get(1).unwrap_or(&0)) << 8)
			| u32::from(*c.get(2).unwrap_or(&0));
		for i in 0..4 {
			if i <= c.len() {
				out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
			} else {
				out.push('=');
			}
		}
	}
	out
}

/// Decodes padded base64; `None` when malformed.
pub fn decode(s: &str) -> Option<Vec<u8>> {
	let b = s.as_bytes();
	if !b.len().is_multiple_of(4) {
		return None;
	}
	let mut out = Vec::with_capacity(b.len() / 4 * 3);
	for (n, c) in b.chunks(4).enumerate() {
		let last = (n + 1) * 4 == b.len();
		let pad = c.iter().rev().take_while(|&&x| x == b'=').count();
		if pad > 2 || (pad > 0 && !last) {
			return None;
		}
		let mut v = 0u32;
		for &x in &c[..4 - pad] {
			v = v << 6 | ALPHABET.iter().position(|&a| a == x)? as u32;
		}
		v <<= 6 * pad as u32;
		let bytes = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
		out.extend_from_slice(&bytes[..3 - pad]);
	}
	Some(out)
}
