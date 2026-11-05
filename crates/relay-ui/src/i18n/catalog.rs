//! The embedded catalogs and the Fluent lookup over them.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::RwLock;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};

use super::Language;

type Bundle = FluentBundle<FluentResource>;

/// The section files every language folder has, in `localizations/<lang>/`.
/// A section is one area of the app; an id is defined in exactly one of them.
pub const SECTIONS: [&str; 10] = [
	"common",
	"calls",
	"peers",
	"audio",
	"channels",
	"settings",
	"messages",
	"tray",
	"notifications",
	"errors",
];

macro_rules! language_sections {
	($dir:literal) => {
		[
			include_str!(concat!("../../../../localizations/", $dir, "/common.ftl")),
			include_str!(concat!("../../../../localizations/", $dir, "/calls.ftl")),
			include_str!(concat!("../../../../localizations/", $dir, "/peers.ftl")),
			include_str!(concat!("../../../../localizations/", $dir, "/audio.ftl")),
			include_str!(concat!("../../../../localizations/", $dir, "/channels.ftl")),
			include_str!(concat!("../../../../localizations/", $dir, "/settings.ftl")),
			include_str!(concat!("../../../../localizations/", $dir, "/messages.ftl")),
			include_str!(concat!("../../../../localizations/", $dir, "/tray.ftl")),
			include_str!(concat!(
				"../../../../localizations/",
				$dir,
				"/notifications.ftl"
			)),
			include_str!(concat!("../../../../localizations/", $dir, "/errors.ftl")),
		]
	};
}

/// The embedded section files of `language`, in [`SECTIONS`] order: the
/// root `localizations/<lang>/` folders, compiled into the binary.
pub fn embedded_sections(language: Language) -> [&'static str; SECTIONS.len()] {
	match language {
		Language::En => language_sections!("en"),
		Language::De => language_sections!("de"),
		Language::Hi => language_sections!("hi"),
		Language::Ja => language_sections!("ja"),
		Language::Ko => language_sections!("ko"),
		Language::PtBr => language_sections!("pt-BR"),
		Language::Ru => language_sections!("ru"),
		Language::Es => language_sections!("es"),
		Language::ZhCn => language_sections!("zh-CN"),
	}
}

/// All sections of `language` merged into one source (one bundle per language).
pub fn embedded_source(language: Language) -> String {
	embedded_sections(language).join("\n")
}

/// Parse one catalog. Entries that fail to parse are skipped (the rest still
/// load); the errors are returned for the catalog checks.
pub fn parse(source: &str) -> (FluentResource, Vec<String>) {
	match FluentResource::try_new(source.to_owned()) {
		Ok(resource) => (resource, Vec::new()),
		Err((resource, errors)) => (resource, errors.iter().map(|e| format!("{e:?}")).collect()),
	}
}

/// A value for a message argument.
pub type Value<'a> = FluentValue<'a>;

/// All languages' bundles plus a cache of the argument-free messages, which are
/// the ones drawn every frame. A cached text is leaked once per (language,
/// message): bounded by the catalog size, and it lets a lookup hand out
/// `&'static str` with no per-frame allocation.
pub struct Catalogs {
	bundles: Vec<Bundle>,
	cache: RwLock<HashMap<(Language, &'static str), &'static str>>,
}

impl Catalogs {
	/// The catalogs compiled into the binary.
	pub fn embedded() -> Self {
		let sources = Language::ALL.map(|l| (l, embedded_source(l)));
		Self::from_sources(sources.iter().map(|(l, s)| (*l, s.as_str())))
	}

	/// Build from explicit sources (a language not listed has no messages and
	/// falls back to English). Used by tests to install a fake translation.
	pub fn from_sources<'a>(sources: impl IntoIterator<Item = (Language, &'a str)>) -> Self {
		let mut bundles: Vec<Bundle> = Language::ALL
			.iter()
			.map(|l| {
				let mut bundle = FluentBundle::new_concurrent(vec![l.langid()]);
				// No Unicode isolation marks around arguments: egui would draw them.
				bundle.set_use_isolating(false);
				bundle
			})
			.collect();
		for (language, source) in sources {
			let (resource, _) = parse(source);
			// A duplicate id keeps the first definition; the checks report it.
			let _ = bundles[language.index()].add_resource(resource);
		}
		Self {
			bundles,
			cache: RwLock::default(),
		}
	}

	fn bundle(&self, language: Language) -> &Bundle {
		&self.bundles[language.index()]
	}

	/// Whether `language` itself (no fallback) defines `id`.
	pub fn has(&self, language: Language, id: &str) -> bool {
		self.bundle(language).has_message(id)
	}

	/// An argument-free message in `language`, English where it is missing,
	/// the id itself where English lacks it too.
	pub fn text(&self, language: Language, id: &'static str) -> &'static str {
		if let Some(hit) = self
			.cache
			.read()
			.ok()
			.and_then(|c| c.get(&(language, id)).copied())
		{
			return hit;
		}
		let text = self
			.format(language, id, None)
			.or_else(|| self.format(Language::FALLBACK, id, None))
			.map_or(id, |t| Box::leak(t.into_boxed_str()));
		if let Ok(mut cache) = self.cache.write() {
			cache.insert((language, id), text);
		}
		text
	}

	/// A message with arguments. A translation that fails to format (a wrong
	/// variable name) falls back to English like a missing one.
	pub fn text_with(&self, language: Language, id: &str, args: &[(&str, Value<'_>)]) -> String {
		let mut fluent_args = FluentArgs::new();
		for (name, value) in args {
			fluent_args.set(*name, value.clone());
		}
		self.format(language, id, Some(&fluent_args))
			.or_else(|| self.format(Language::FALLBACK, id, Some(&fluent_args)))
			.unwrap_or_else(|| id.to_owned())
	}

	/// `None` when the message is missing or does not format cleanly.
	fn format(
		&self,
		language: Language,
		id: &str,
		args: Option<&FluentArgs<'_>>,
	) -> Option<String> {
		let bundle = self.bundle(language);
		let pattern = bundle.get_message(id)?.value()?;
		let mut errors = Vec::new();
		let text = bundle.format_pattern(pattern, args, &mut errors);
		if errors.is_empty() || language == Language::FALLBACK {
			Some(Cow::into_owned(text))
		} else {
			None
		}
	}
}
