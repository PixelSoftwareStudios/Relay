//! The embedded catalogs and the Fluent lookup over them.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::RwLock;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};

use super::{Language, embed};

type Bundle = FluentBundle<FluentResource>;

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
		let sources = Language::ALL.map(|l| (l, embed::source(l)));
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
