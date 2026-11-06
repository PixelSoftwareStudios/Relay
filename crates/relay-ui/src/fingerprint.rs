//! The full fingerprint of a device (its key digest, 64 hex digits) as people read and type
//! it: in groups, with spaces or dashes, in any case.

/// Hex digits per group.
const GROUP: usize = 4;
/// Groups per line.
const PER_LINE: usize = 4;
/// Digits of a full fingerprint.
pub const DIGITS: usize = 64;

/// The hex digits of `typed`, lower case, with spaces, dashes and colons dropped. Anything
/// else stays, so it cannot match.
pub fn digits(typed: &str) -> String {
	typed
		.chars()
		.filter(|c| !matches!(c, ' ' | '-' | ':' | '\t' | '\n' | '\r'))
		.map(|c| c.to_ascii_lowercase())
		.collect()
}

/// Whether `typed` is exactly the full fingerprint `full` (64 hex digits). A prefix or a
/// short form never matches.
pub fn matches(full: &str, typed: &str) -> bool {
	let typed = digits(typed);
	typed.len() == DIGITS && typed.eq_ignore_ascii_case(full)
}

/// The fingerprint in lines of four groups of four digits, upper case, for reading aloud.
pub fn lines(full: &str) -> Vec<String> {
	let groups: Vec<String> = full
		.to_ascii_uppercase()
		.as_bytes()
		.chunks(GROUP)
		.map(|c| String::from_utf8_lossy(c).into_owned())
		.collect();
	groups.chunks(PER_LINE).map(|g| g.join(" ")).collect()
}
