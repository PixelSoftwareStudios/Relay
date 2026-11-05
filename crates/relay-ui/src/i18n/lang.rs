//! The supported UI languages: codes, native names and the picker order.

use unic_langid::LanguageIdentifier;

/// A UI language. The variant order is the picker order: English first (the
/// default fallback), then the others alphabetically by their English name
/// (German, Hindi, Japanese, Korean, Portuguese, Russian, Spanish, Chinese).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
	En,
	De,
	Hi,
	Ja,
	Ko,
	PtBr,
	Ru,
	Es,
	ZhCn,
}

impl Language {
	/// Every language in picker order.
	pub const ALL: [Language; 9] = [
		Language::En,
		Language::De,
		Language::Hi,
		Language::Ja,
		Language::Ko,
		Language::PtBr,
		Language::Ru,
		Language::Es,
		Language::ZhCn,
	];

	/// The fallback language: every missing message is looked up here.
	pub const FALLBACK: Language = Language::En;

	/// BCP 47 code; also the stem of the catalog file `assets/i18n/<code>.ftl`
	/// and the value stored in the desktop preferences.
	pub const fn code(self) -> &'static str {
		match self {
			Self::En => "en",
			Self::De => "de",
			Self::Hi => "hi",
			Self::Ja => "ja",
			Self::Ko => "ko",
			Self::PtBr => "pt-BR",
			Self::Ru => "ru",
			Self::Es => "es",
			Self::ZhCn => "zh-CN",
		}
	}

	/// The language's name in that language, as the picker shows it.
	pub const fn native_name(self) -> &'static str {
		match self {
			Self::En => "English",
			Self::De => "Deutsch",
			Self::Hi => "हिन्दी",
			Self::Ja => "日本語",
			Self::Ko => "한국어",
			Self::PtBr => "Português (Brasil)",
			Self::Ru => "Русский",
			Self::Es => "Español",
			Self::ZhCn => "简体中文",
		}
	}

	/// Position in [`Language::ALL`].
	pub const fn index(self) -> usize {
		self as usize
	}

	/// Parse a stored code (exact, case-insensitive). Not for system locales:
	/// see [`super::locale::match_locale`].
	pub fn from_code(code: &str) -> Option<Self> {
		Self::ALL
			.into_iter()
			.find(|l| l.code().eq_ignore_ascii_case(code.trim()))
	}

	/// The language tag, for Fluent plural rules.
	pub fn langid(self) -> LanguageIdentifier {
		self.code()
			.parse()
			.unwrap_or_else(|_| LanguageIdentifier::default())
	}
}
