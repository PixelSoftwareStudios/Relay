//! Plain-language wording for storage refusals the engine reports in technical words.

use crate::tr;

/// The engine's marker for a full trust table, followed by the limit.
const TRUST_FULL: &str = "trust limit reached: ";

/// Engine messages that mean the database could not take a change.
const STORAGE_FAILURES: &[&str] = &["database error", "could not save the device"];

/// What a person reads for an engine refusal about saved data, or `None` for any other
/// refusal. A storage failure is logged with its detail and shown without it.
pub fn storage_words(message: &str) -> Option<String> {
	if let Some(limit) = message
		.split_once(TRUST_FULL)
		.and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit()).next())
		.filter(|digits| !digits.is_empty())
	{
		return Some(tr!("err-trust-full", limit = limit));
	}
	let lower = message.to_lowercase();
	if STORAGE_FAILURES.iter().any(|marker| lower.contains(marker)) {
		tracing::warn!(detail = message, "a change could not be saved");
		return Some(tr!("err-save-failed").to_owned());
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_full_table_names_its_limit_and_a_fault_does_not_say_full() {
		let full = storage_words("Unavailable: trust limit reached: 4096").unwrap();
		assert!(full.contains("4096") && full.contains("Contacts"), "{full}");
		let fault = storage_words("database error: no such table: favorites").unwrap();
		assert_eq!(fault, tr!("err-save-failed"));
		assert!(!fault.contains("favorites"));
		assert!(storage_words("could not reach a device").is_none());
	}
}
