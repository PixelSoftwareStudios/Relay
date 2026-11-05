//! RFC 3492 decoder, used only to display internationalised hosts.

const BASE: u32 = 36;
const T_MIN: u32 = 1;
const T_MAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;

fn adapt(mut delta: u32, num: u32, first: bool) -> u32 {
	delta /= if first { DAMP } else { 2 };
	delta += delta / num;
	let mut k = 0;
	while delta > ((BASE - T_MIN) * T_MAX) / 2 {
		delta /= BASE - T_MIN;
		k += BASE;
	}
	k + (BASE - T_MIN + 1) * delta / (delta + SKEW)
}

fn digit(b: u8) -> Option<u32> {
	match b {
		b'a'..=b'z' => Some(u32::from(b - b'a')),
		b'A'..=b'Z' => Some(u32::from(b - b'A')),
		b'0'..=b'9' => Some(u32::from(b - b'0') + 26),
		_ => None,
	}
}

/// Decodes the part after `xn--`. `None` on malformed input or overflow.
pub fn decode(input: &str) -> Option<String> {
	if !input.is_ascii() || input.len() > 63 {
		return None;
	}
	let (basic, rest) = match input.rfind('-') {
		Some(i) => (&input[..i], &input[i + 1..]),
		None => ("", input),
	};
	let mut out: Vec<char> = basic.chars().collect();
	let (mut n, mut i, mut bias) = (128u32, 0u32, 72u32);
	let bytes = rest.as_bytes();
	let mut pos = 0;
	while pos < bytes.len() {
		let old = i;
		let (mut w, mut k) = (1u32, BASE);
		loop {
			let d = digit(*bytes.get(pos)?)?;
			pos += 1;
			i = i.checked_add(d.checked_mul(w)?)?;
			let t = if k <= bias {
				T_MIN
			} else if k >= bias + T_MAX {
				T_MAX
			} else {
				k - bias
			};
			if d < t {
				break;
			}
			w = w.checked_mul(BASE - t)?;
			k += BASE;
		}
		let len = u32::try_from(out.len()).ok()? + 1;
		bias = adapt(i - old, len, old == 0);
		n = n.checked_add(i / len)?;
		i %= len;
		out.insert(i as usize, char::from_u32(n)?);
		i += 1;
	}
	Some(out.into_iter().collect())
}
