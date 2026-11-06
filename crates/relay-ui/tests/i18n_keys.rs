//! Every message id the code uses exists in English, and English has no id
//! the code never uses.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use relay_ui::i18n::check::messages;
use relay_ui::i18n::{Language, embedded_sections};

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
		// The macro's own docs use a made-up id.
		.filter(|p| !p.ends_with("i18n/mod.rs"))
		.map(|p| {
			let text = std::fs::read_to_string(&p).unwrap();
			(p, text)
		})
		.collect()
}

/// Ids in `tr!("id"...)` calls (whitespace after the parenthesis allowed).
fn referenced(text: &str) -> BTreeSet<String> {
	let mut ids = BTreeSet::new();
	for opener in ["tr!("] {
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
		.flat_map(|(_, s)| messages(s).into_keys())
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
fn strings_are_only_looked_up_through_tr() {
	let direct: Vec<_> = sources()
		.into_iter()
		.filter(|(_, text)| text.contains("i18n::t(") || text.contains("i18n::t_with("))
		.map(|(p, _)| p.display().to_string())
		.collect();
	assert!(
		direct.is_empty(),
		"use tr! instead of i18n::t in: {direct:?}"
	);
}
