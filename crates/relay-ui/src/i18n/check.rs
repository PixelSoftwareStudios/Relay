//! Catalog checks, reusable by translators: `cargo test -p relay-ui --test
//! i18n_catalogs` runs [`problems`] over every embedded catalog and
//! [`layout_problems`] over the `localizations` folder.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::Language;
use super::catalog::{SECTIONS, embedded_sections, parse};

/// Message id to the set of `$variables` its text uses.
pub type Messages = BTreeMap<String, BTreeSet<String>>;

/// The messages of one section source and their variables. An entry starts at
/// a line beginning with an identifier and `=`; indented lines continue it.
pub fn messages(source: &str) -> Messages {
	let mut out = Messages::new();
	let mut current: Option<String> = None;
	for line in source.lines() {
		let starts_entry = line.starts_with(|c: char| c.is_ascii_alphabetic());
		if starts_entry && let Some((id, rest)) = line.split_once('=') {
			let id = id.trim().to_owned();
			out.insert(id.clone(), variables(rest));
			current = Some(id);
		} else if line.starts_with(char::is_whitespace) {
			if let Some(id) = &current {
				out.entry(id.clone()).or_default().extend(variables(line));
			}
		} else {
			current = None;
		}
	}
	out
}

/// The `$name` references in a piece of message text.
fn variables(text: &str) -> BTreeSet<String> {
	let mut vars = BTreeSet::new();
	let mut rest = text;
	while let Some(at) = rest.find('$') {
		rest = &rest[at + 1..];
		let name: String = rest
			.chars()
			.take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
			.collect();
		if !name.is_empty() {
			vars.insert(name);
		}
	}
	vars
}

/// Every id of a language with the section that defines it. An id defined in
/// two sections is reported in `problems` and keeps its first section.
fn index(
	language: &str,
	sections: &[(&str, &str)],
	problems: &mut Vec<String>,
) -> BTreeMap<String, (String, BTreeSet<String>)> {
	let mut all: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
	for (section, source) in sections {
		for error in parse(source).1 {
			problems.push(format!("{language}/{section}.ftl: syntax error {error}"));
		}
		let ids = source
			.lines()
			.filter(|l| l.starts_with(|c: char| c.is_ascii_alphabetic()))
			.filter_map(|l| l.split_once('=').map(|(id, _)| id.trim()));
		let mut seen = BTreeSet::new();
		for id in ids {
			if !seen.insert(id) {
				problems.push(format!("{language}/{section}.ftl: {id} is defined twice"));
			}
		}
		for (id, vars) in messages(source) {
			if let Some((other, _)) = all.get(&id) {
				if other != section {
					problems.push(format!(
						"{language}/{section}.ftl: {id} is also defined in {other}.ftl"
					));
				}
			} else {
				all.insert(id, ((*section).to_owned(), vars));
			}
		}
	}
	all
}

/// Problems of one translation against the English reference, given as
/// `(section, source)` pairs: a source that does not parse, an id defined in
/// two sections, an id English lacks (or defines in another section), or
/// variables that differ from English's.
pub fn translation_problems(
	reference: &[(&str, &str)],
	language: &str,
	sections: &[(&str, &str)],
) -> Vec<String> {
	let mut out = Vec::new();
	let english = index("en", reference, &mut Vec::new());
	for (id, (section, vars)) in index(language, sections, &mut out) {
		match english.get(&id) {
			None => out.push(format!(
				"{language}/{section}.ftl: {id} does not exist in en"
			)),
			Some((want_section, _)) if *want_section != section => out.push(format!(
				"{language}/{section}.ftl: {id} belongs in {want_section}.ftl (as in en)"
			)),
			Some((_, want)) if *want != vars => out.push(format!(
				"{language}/{section}.ftl: {id} uses variables {vars:?} but en uses {want:?}"
			)),
			Some(_) => {}
		}
	}
	out
}

fn embedded(language: Language) -> Vec<(&'static str, &'static str)> {
	SECTIONS
		.into_iter()
		.zip(embedded_sections(language))
		.collect()
}

/// Every content problem of every embedded catalog (English included).
pub fn problems() -> Vec<String> {
	let reference = embedded(Language::En);
	let mut out = Vec::new();
	index("en", &reference, &mut out);
	for language in Language::ALL.into_iter().skip(1) {
		out.extend(translation_problems(
			&reference,
			language.code(),
			&embedded(language),
		));
	}
	out
}

/// Problems with the folder layout under `root` (the repo's `localizations`):
/// every language has a folder with exactly the section files English has.
pub fn layout_problems(root: &Path) -> Vec<String> {
	let files = |dir: &Path| -> BTreeSet<String> {
		std::fs::read_dir(dir)
			.map(|rd| {
				rd.filter_map(Result::ok)
					.map(|e| e.file_name().to_string_lossy().into_owned())
					.collect()
			})
			.unwrap_or_default()
	};
	let expected: BTreeSet<String> = SECTIONS.iter().map(|s| format!("{s}.ftl")).collect();
	let mut out = Vec::new();
	let folders = files(root);
	let wanted: BTreeSet<String> = Language::ALL.iter().map(|l| l.code().to_owned()).collect();
	for extra in folders.difference(&wanted) {
		out.push(format!("localizations/{extra} is not a supported language"));
	}
	for language in Language::ALL {
		let found = files(&root.join(language.code()));
		for missing in expected.difference(&found) {
			out.push(format!(
				"localizations/{}/{missing} is missing",
				language.code()
			));
		}
		for extra in found.difference(&expected) {
			out.push(format!(
				"localizations/{}/{extra} is not a known section",
				language.code()
			));
		}
	}
	out
}
