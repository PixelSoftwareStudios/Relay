//! The language list, the picker and system-locale matching.

use relay_ui::i18n::locale::{match_locale, resolve};
use relay_ui::i18n::{Language, picker};

#[test]
fn the_picker_lists_english_first_then_the_others_by_english_name() {
	let codes: Vec<_> = picker::entries(Language::En)
		.map(|(l, _)| l.map(Language::code))
		.collect();
	assert_eq!(
		codes,
		[
			None,
			Some("en"),
			Some("de"),
			Some("hi"),
			Some("ja"),
			Some("ko"),
			Some("pt-BR"),
			Some("ru"),
			Some("es"),
			Some("zh-CN")
		]
	);
	assert_eq!(Language::ALL[0], Language::En);
	assert_eq!(Language::FALLBACK, Language::En);
}

#[test]
fn each_language_is_shown_by_its_own_name() {
	let names: Vec<_> = Language::ALL.iter().map(|l| l.native_name()).collect();
	assert_eq!(
		names,
		[
			"English",
			"Deutsch",
			"हिन्दी",
			"日本語",
			"한국어",
			"Português (Brasil)",
			"Русский",
			"Español",
			"简体中文"
		]
	);
	for (language, label) in picker::entries(Language::En).skip(1) {
		assert!(label.starts_with(language.unwrap().native_name()));
	}
}

#[test]
fn the_first_entry_is_system_with_the_detected_language_by_native_name() {
	let first = |detected| picker::entries(detected).next().unwrap();
	assert_eq!(first(Language::De), (None, "System (Deutsch)".to_owned()));
	// No supported match means English was detected.
	assert_eq!(first(Language::En).1, "System (English)");
	assert_eq!(first(Language::ZhCn).1, "System (简体中文)");
}

#[test]
fn hindi_is_flagged_experimental_in_the_picker() {
	assert!(picker::label(Language::Hi).contains("experimental"));
	assert!(!picker::label(Language::De).contains("experimental"));
}

#[test]
fn codes_round_trip_and_index_is_the_picker_position() {
	for (i, language) in Language::ALL.into_iter().enumerate() {
		assert_eq!(Language::from_code(language.code()), Some(language));
		assert_eq!(language.index(), i);
		assert_eq!(language.langid().to_string(), language.code());
	}
	assert_eq!(Language::from_code("PT-br"), Some(Language::PtBr));
	assert_eq!(Language::from_code("fr"), None);
}

#[test]
fn system_locales_are_matched_to_supported_languages() {
	for (tag, want) in [
		("pt_BR.UTF-8", Some(Language::PtBr)),
		("pt-PT", Some(Language::PtBr)),
		("zh_Hans_CN", Some(Language::ZhCn)),
		("zh-CN", Some(Language::ZhCn)),
		("zh", Some(Language::ZhCn)),
		("zh-SG", Some(Language::ZhCn)),
		("zh_TW.UTF-8", None),
		("zh-Hant", None),
		("de_AT@euro", Some(Language::De)),
		("en_US.UTF-8", Some(Language::En)),
		("ja-JP", Some(Language::Ja)),
		("ko_KR", Some(Language::Ko)),
		("hi_IN", Some(Language::Hi)),
		("ru-RU", Some(Language::Ru)),
		("es-419", Some(Language::Es)),
		("fr_FR", None),
		("C", None),
		("POSIX", None),
		("", None),
	] {
		assert_eq!(match_locale(tag), want, "{tag}");
	}
}

#[test]
fn the_first_supported_preference_wins_and_unknown_means_english() {
	assert_eq!(resolve(["fr-FR", "de-DE", "ja"]), Language::De);
	assert_eq!(resolve(["zh-TW", "ko"]), Language::Ko);
	assert_eq!(resolve(["fr", "it"]), Language::En);
	assert_eq!(resolve([]), Language::En);
}
