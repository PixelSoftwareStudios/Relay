use thiserror::Error;

use super::{NAME_MAX_CHARS, NAME_MIN_CHARS};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NameError {
	#[error("channel name must be {NAME_MIN_CHARS} to {NAME_MAX_CHARS} characters")]
	Length,
	#[error("channel name contains a control, bidi or markup character")]
	Character,
}

/// A validated channel name: bounded, trimmed, without control, bidi, zero-width or markup
/// characters. Names are display text only and never an identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelName(String);

impl ChannelName {
	pub fn new(raw: &str) -> Result<Self, NameError> {
		let trimmed = raw.trim();
		let count = trimmed.chars().count();
		if !(NAME_MIN_CHARS..=NAME_MAX_CHARS).contains(&count) {
			return Err(NameError::Length);
		}
		if trimmed.chars().any(is_forbidden) || bad_combining(trimmed) {
			return Err(NameError::Character);
		}
		Ok(Self(trimmed.to_owned()))
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

fn bad_combining(text: &str) -> bool {
	let mut run = 0;
	for c in text.chars() {
		if is_combining_mark(c) {
			run += 1;
			if run > COMBINING_RUN_MAX {
				return true;
			}
		} else {
			run = 0;
		}
	}
	text.chars().next().is_some_and(is_combining_mark)
}

fn is_forbidden(c: char) -> bool {
	c.is_control()
		|| matches!(c, '<' | '>' | '&' | '"')
		|| is_invisible_or_format(c)
		|| is_private_use_or_noncharacter(c)
		|| c.is_whitespace() && c != ' '
}

/// Characters that render as nothing or change rendering order: the Unicode
/// `Default_Ignorable_Code_Point` set (soft hyphen, combining grapheme joiner, Hangul fillers,
/// Mongolian and variation selectors, zero-width and bidi controls, the deprecated format
/// controls U+206A..206F, the BOM, tag characters ...), plus the braille blank and the line and
/// paragraph separators. Display names and channel names must never contain them.
pub const fn is_invisible_or_format(c: char) -> bool {
	matches!(c as u32,
		0x00AD | 0x034F | 0x061C | 0x115F..=0x1160 | 0x17B4..=0x17B5 | 0x180B..=0x180F
		| 0x2028..=0x2029 | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0x2800
		| 0x3164 | 0xFE00..=0xFE0F | 0xFEFF | 0xFFA0 | 0xFFF0..=0xFFF8
		| 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0000..=0xE0FFF)
}

/// Private-use code points and noncharacters, which have no agreed rendering.
pub const fn is_private_use_or_noncharacter(c: char) -> bool {
	let v = c as u32;
	matches!(v, 0xE000..=0xF8FF | 0xFDD0..=0xFDEF | 0xF0000..=0x10FFFF) || (v & 0xFFFE) == 0xFFFE
}

/// Combining marks (common blocks). A name may not start with one, and runs are bounded.
pub const fn is_combining_mark(c: char) -> bool {
	matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Longest run of combining marks after one base character that a name may contain.
pub const COMBINING_RUN_MAX: usize = 2;
