//! Text, display-name and file-name sanitization.

use crate::limits::{
	MAX_DISPLAY_NAME_BYTES, MAX_EXTENSION_BYTES, MAX_FILE_NAME_BYTES, MAX_TEXT_BYTES,
};

/// Why message text was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
	/// Empty after normalisation.
	#[error("message is empty")]
	Empty,
	/// Over the byte limit.
	#[error("message is {len} bytes, limit {max}")]
	TooLong {
		/// Actual size.
		len: usize,
		/// Limit.
		max: usize,
	},
	/// A control character other than newline and tab.
	#[error("control character U+{0:04X}")]
	Control(u32),
	/// A bidi override, zero-width or other format character.
	#[error("invisible or direction-changing character U+{0:04X}")]
	Format(u32),
}

/// True for characters of Unicode category Cf and other invisible ones we refuse.
pub fn is_format_char(c: char) -> bool {
	matches!(c as u32,
		0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x08E2 | 0x180E
		| 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F
		| 0xFEFF | 0xFFF9..=0xFFFB | 0x110BD | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A
		| 0xE0001 | 0xE0020..=0xE007F)
}

/// Validates message text, normalising `\r\n` to `\n`. Idempotent.
pub fn sanitize_text(input: &str) -> Result<String, TextError> {
	sanitize_with_limit(input, MAX_TEXT_BYTES)
}

/// Like [`sanitize_text`] with a caller-chosen byte limit (previews use a larger one).
pub fn sanitize_with_limit(input: &str, max: usize) -> Result<String, TextError> {
	if input.len() > max.saturating_mul(2) {
		return Err(TextError::TooLong {
			len: input.len(),
			max,
		});
	}
	let mut out = String::with_capacity(input.len());
	let chars: Vec<char> = input.chars().collect();
	for (i, &c) in chars.iter().enumerate() {
		match c {
			'\n' | '\t' => out.push(c),
			'\r' if chars.get(i + 1) == Some(&'\n') => {}
			'\u{200D}' => {
				let before = i.checked_sub(1).map(|j| chars[j]);
				let after = chars.get(i + 1).copied();
				let ok = |c: Option<char>| c.is_some_and(|c| !c.is_ascii() && !is_format_char(c));
				if ok(before) && ok(after) {
					out.push(c);
				} else {
					return Err(TextError::Format(c as u32));
				}
			}
			c if c.is_control() => return Err(TextError::Control(c as u32)),
			c if is_format_char(c) => return Err(TextError::Format(c as u32)),
			c => out.push(c),
		}
	}
	if out.len() > max {
		return Err(TextError::TooLong {
			len: out.len(),
			max,
		});
	}
	if out.trim().is_empty() {
		return Err(TextError::Empty);
	}
	Ok(out)
}

/// Strips controls and format characters, collapses whitespace and bounds the length.
pub fn sanitize_display_name(input: &str) -> String {
	let cleaned: String = input
		.chars()
		.filter(|c| !c.is_control() && !is_format_char(*c))
		.collect();
	let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
	truncate_chars(&joined, MAX_DISPLAY_NAME_BYTES)
}

fn truncate_chars(s: &str, max: usize) -> String {
	if s.len() <= max {
		return s.to_owned();
	}
	let mut end = max;
	while !s.is_char_boundary(end) {
		end -= 1;
	}
	s[..end].to_owned()
}

/// A file name that is safe as a single path component.
#[derive(Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SafeFileName(String);

impl SafeFileName {
	/// The name.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl std::fmt::Debug for SafeFileName {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "SafeFileName({:?})", self.0)
	}
}

impl std::fmt::Display for SafeFileName {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.0)
	}
}

fn is_reserved(stem: &str) -> bool {
	let up = stem.to_ascii_uppercase();
	matches!(up.as_str(), "CON" | "PRN" | "AUX" | "NUL")
		|| ((up.starts_with("COM") || up.starts_with("LPT"))
			&& up.len() == 4
			&& matches!(up.as_bytes()[3], b'1'..=b'9'))
}

/// Reduces an untrusted name to a safe single component. Never fails; idempotent.
pub fn sanitize_file_name(input: &str) -> SafeFileName {
	let last = input.rsplit(['/', '\\']).next().unwrap_or("");
	let mut s: String = last
		.chars()
		.filter(|c| !c.is_control() && !is_format_char(*c))
		.map(|c| {
			if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
				'_'
			} else {
				c
			}
		})
		.collect();
	s = s
		.trim_start_matches(['.', ' '])
		.trim_end_matches(['.', ' '])
		.to_owned();
	if s.len() > MAX_FILE_NAME_BYTES {
		s = shorten_keeping_extension(&s);
		s = s.trim_end_matches(['.', ' ']).to_owned();
	}
	if s.is_empty() {
		s = "file".to_owned();
	}
	let stem = s.split('.').next().unwrap_or("");
	if is_reserved(stem) {
		s.insert(0, '_');
	}
	SafeFileName(s)
}

fn shorten_keeping_extension(s: &str) -> String {
	let ext = match s.rfind('.') {
		Some(i) if s.len() - i <= MAX_EXTENSION_BYTES + 1 && i > 0 => &s[i..],
		_ => "",
	};
	let stem_max = MAX_FILE_NAME_BYTES - ext.len();
	let stem = &s[..s.len() - ext.len()];
	format!("{}{}", truncate_chars(stem, stem_max), ext)
}

/// Advisory media type from the extension. Never trusted for decisions.
pub fn guess_media_type(name: &str) -> &'static str {
	let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
	match ext.as_str() {
		"txt" | "log" | "md" => "text/plain",
		"json" => "application/json",
		"png" => "image/png",
		"jpg" | "jpeg" => "image/jpeg",
		"gif" => "image/gif",
		"webp" => "image/webp",
		"pdf" => "application/pdf",
		"zip" => "application/zip",
		"csv" => "text/csv",
		_ => "application/octet-stream",
	}
}

/// True when the extension is one that runs code on common desktops (UI warning only).
pub fn looks_executable(name: &str) -> bool {
	let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
	matches!(
		ext.as_str(),
		"exe"
			| "msi" | "bat"
			| "cmd" | "com"
			| "scr" | "ps1"
			| "vbs" | "js"
			| "jar" | "sh"
			| "app" | "dmg"
			| "pkg" | "deb"
			| "rpm" | "apk"
			| "lnk" | "appimage"
			| "command"
	)
}
