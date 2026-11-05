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
		if trimmed.chars().any(is_forbidden) {
			return Err(NameError::Character);
		}
		Ok(Self(trimmed.to_owned()))
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

fn is_forbidden(c: char) -> bool {
	c.is_control()
		|| matches!(
			c,
			'<' | '>' | '&' | '"' | '\u{061C}' | '\u{00AD}' | '\u{FEFF}'
		) || matches!(c,
			'\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}'
			| '\u{2066}'..='\u{2069}')
}
