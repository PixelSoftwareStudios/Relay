//! Every message id the code uses exists in English, and English has no id
//! the code never uses.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use relay_ui::i18n::check::messages;
use relay_ui::i18n::{Language, SECTIONS, embedded_sections};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
	for entry in std::fs::read_dir(dir).unwrap().flatten() {
		let path = entry.path();
		if path.is_dir() {
			rust_files(&path, out);
		} else if path.extension().is_some_and(|e| e == "rs") {
			out.push(path);
		}
	}
}

fn sources() -> Vec<(PathBuf, String)> {
	let base = Path::new(env!("CARGO_MANIFEST_DIR"));
	let mut files = Vec::new();
	rust_files(&base.join("src"), &mut files);
	rust_files(&base.join("../../apps/relay-desktop/src"), &mut files);
	assert!(files.len() > 30, "found only {} source files", files.len());
	files
		.into_iter()
		.filter(|p| !p.components().any(|c| c.as_os_str() == "i18n"))
		.map(|p| {
			let text = std::fs::read_to_string(&p).unwrap();
			(p, text)
		})
		.collect()
}

/// Ids in `tr!("id"...)` calls (whitespace after the parenthesis allowed), and
/// in the helpers that take an id literal.
fn referenced(text: &str) -> BTreeSet<String> {
	let mut ids = BTreeSet::new();
	for opener in ["tr!(", "blocked("] {
		let mut rest = text;
		while let Some(at) = rest.find(opener) {
			rest = rest[at + opener.len()..].trim_start();
			if let Some(literal) = rest.strip_prefix('"')
				&& let Some(end) = literal.find('"')
			{
				ids.insert(literal[..end].to_owned());
			}
		}
	}
	ids
}

fn english() -> BTreeSet<String> {
	embedded_sections(Language::En)
		.iter()
		.flat_map(|s| messages(s).into_keys())
		.collect()
}

#[test]
fn every_referenced_key_exists_in_english() {
	let en = english();
	let mut missing = Vec::new();
	for (path, text) in sources() {
		for id in referenced(&text) {
			if !en.contains(&id) {
				missing.push(format!("{}: {id}", path.display()));
			}
		}
	}
	assert!(
		missing.is_empty(),
		"ids missing from en:\n{}",
		missing.join("\n")
	);
}

#[test]
fn english_has_no_unused_key() {
	let all: String = sources().into_iter().map(|(_, t)| t).collect();
	let unused: Vec<_> = english()
		.into_iter()
		.filter(|id| !all.contains(&format!("\"{id}\"")))
		.collect();
	assert!(unused.is_empty(), "unused ids in en: {unused:?}");
}

#[test]
fn the_sections_are_the_known_ones() {
	assert_eq!(SECTIONS.len(), embedded_sections(Language::En).len());
}
