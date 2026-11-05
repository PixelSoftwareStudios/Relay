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
	match id.language.as_str() {
		"en" => Some(Language::En),
		"de" => Some(Language::De),
		"hi" => Some(Language::Hi),
		"ja" => Some(Language::Ja),
		"ko" => Some(Language::Ko),
		"pt" => Some(Language::PtBr),
		"ru" => Some(Language::Ru),
		"es" => Some(Language::Es),
		"zh" => {
			let traditional = id.script.is_some_and(|s| s.as_str() == "Hant")
				|| id
					.region
					.is_some_and(|r| matches!(r.as_str(), "TW" | "HK" | "MO"));
			(!traditional).then_some(Language::ZhCn)
		}
		_ => None,
	}
}

/// The first supported language among `preferred` locales (most wanted
/// first), else English.
pub fn resolve<'a>(preferred: impl IntoIterator<Item = &'a str>) -> Language {
	preferred
		.into_iter()
		.find_map(match_locale)
		.unwrap_or(Language::FALLBACK)
}
