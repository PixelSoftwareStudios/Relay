//! What the language picker in Settings shows, and in which order.

use super::Language;
use crate::tr;

/// Hindi is drawn without conjuncts (egui does no complex text shaping), so
/// it is flagged in the picker until the renderer can shape Devanagari.
pub const EXPERIMENTAL: [Language; 1] = [Language::Hi];

/// The picker entry for `language`: its native name, plus a marker for the
/// experimental ones.
pub fn label(language: Language) -> &'static str {
	match language {
		Language::Hi => "हिन्दी (experimental)",
		other => other.native_name(),
	}
}

/// The first entry, "System (Deutsch)": follow the system language, shown with
/// the one detected now by its native name.
pub fn system_label(detected: Language) -> String {
	tr!("language-system", language = detected.native_name())
}

/// The entry of a choice: `None` is "System".
pub fn choice_label(choice: Option<Language>, detected: Language) -> String {
	choice.map_or_else(|| system_label(detected), |l| label(l).to_owned())
}

/// All picker entries in display order: "System" (`None`) first, then
/// [`Language::ALL`].
pub fn entries(detected: Language) -> impl Iterator<Item = (Option<Language>, String)> {
	std::iter::once(None)
		.chain(Language::ALL.into_iter().map(Some))
		.map(move |choice| (choice, choice_label(choice, detected)))
}
