//! What the language picker in Settings shows, and in which order.

use super::Language;

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

/// All picker entries in display order.
pub fn entries() -> impl Iterator<Item = (Language, &'static str)> {
	Language::ALL.into_iter().map(|l| (l, label(l)))
}
