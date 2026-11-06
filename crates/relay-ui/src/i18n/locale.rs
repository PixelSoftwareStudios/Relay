//! Matching system locales (`pt_BR.UTF-8`, `zh_Hans_CN`, `de-AT`) to the
//! supported languages.

use unic_langid::LanguageIdentifier;

use super::Language;

/// The supported language for one locale tag, or `None` when there is none.
///
/// POSIX decorations (`.UTF-8`, `@euro`) are dropped and `_` is read as `-`.
/// Portuguese of any region maps to `pt-BR` (the only Portuguese catalog).
/// Chinese maps to `zh-CN` unless it is explicitly Traditional (script
/// `Hant`, or region TW, HK or MO), which has no catalog.
pub fn match_locale(tag: &str) -> Option<Language> {
	let bare = tag.split(['.', '@']).next().unwrap_or("").replace('_', "-");
	let id: LanguageIdentifier = bare.parse().ok()?;
	let language = id.language.as_str();
	let traditional = id.script.is_some_and(|s| s.as_str() == "Hant")
		|| id
			.region
			.is_some_and(|r| matches!(r.as_str(), "TW" | "HK" | "MO"));
	if language == "zh" && traditional {
		return None;
	}
	// The language subtag of a supported code (`pt` of `pt-BR`) claims every
	// region of it, so a new language needs no entry here.
	Language::ALL
		.into_iter()
		.find(|l| l.code().split('-').next() == Some(language))
}

/// The first supported language among `preferred` locales (most wanted
/// first), else English.
pub fn resolve<'a>(preferred: impl IntoIterator<Item = &'a str>) -> Language {
	preferred
		.into_iter()
		.find_map(match_locale)
		.unwrap_or(Language::FALLBACK)
}
