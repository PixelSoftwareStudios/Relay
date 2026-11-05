//! Display-name hygiene: what a peer may call itself, and when two names look alike.
//!
//! Names are display text only; the fingerprint is the identity. The defence here is against
//! spoofing the trust prompt: a name that renders like a trusted contact's must not slip past
//! the key-change alarm just by using an invisible or lookalike character.

use relay_core::channel::{
	COMBINING_RUN_MAX, is_combining_mark, is_invisible_or_format, is_private_use_or_noncharacter,
};
use unicode_normalization::UnicodeNormalization;
use unicode_security::confusable_detection::skeleton;

use crate::limits::{DISPLAY_NAME_MAX_BYTES, DISPLAY_NAME_MAX_CHARS};

const FALLBACK: &str = "Unnamed device";

/// NFKC-normalises, then removes control characters, every invisible or format character
/// (zero-width, bidi, Hangul fillers, variation selectors, tag characters, the deprecated
/// U+206A..206F, braille blank, line and paragraph separators), private-use code points,
/// markup delimiters and leading or excessive combining marks. Whitespace becomes single
/// spaces. The result is bounded to 64 scalar values and 256 UTF-8 bytes.
pub fn sanitize_display_name(raw: &str) -> String {
	let mut out = String::new();
	let mut run = 0usize;
	let mut pending_space = false;
	for c in raw.chars().nfkc() {
		if c.is_whitespace() {
			pending_space = !out.is_empty();
			run = 0;
			continue;
		}
		let dropped = c.is_control()
			|| is_invisible_or_format(c)
			|| is_private_use_or_noncharacter(c)
			|| matches!(c, '<' | '>' | '&' | '"');
		if dropped {
			continue;
		}
		if is_combining_mark(c) {
			if out.is_empty() || run >= COMBINING_RUN_MAX {
				continue;
			}
			run += 1;
		} else {
			run = 0;
		}
		let needed = 1 + usize::from(pending_space);
		if out.chars().count() + needed > DISPLAY_NAME_MAX_CHARS {
			break;
		}
		if pending_space {
			out.push(' ');
			pending_space = false;
		}
		out.push(c);
	}
	let out = truncate(out.trim(), DISPLAY_NAME_MAX_BYTES);
	if out.is_empty() {
		FALLBACK.to_owned()
	} else {
		out
	}
}

/// A comparison key under which lookalike names are equal: the sanitized name run through the
/// Unicode TR39 confusable skeleton, lowercased, with spaces removed. `Alice`, `Alice` with a
/// Cyrillic `А`, `ALICE` and `A lice` share a skeleton. Use it to decide "is this the same name
/// as a contact I already trust"; never to display.
pub fn name_skeleton(name: &str) -> String {
	let sanitized = sanitize_display_name(name).to_lowercase();
	skeleton(&sanitized)
		.collect::<String>()
		.to_lowercase()
		.chars()
		.filter(|c| !c.is_whitespace())
		.collect()
}

fn truncate(text: &str, max_bytes: usize) -> String {
	let mut end = text.len().min(max_bytes);
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	text[..end].to_owned()
}
