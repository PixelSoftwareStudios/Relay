//! Every character in every shipped translation has a glyph in the bundled fonts.

use std::fs;
use std::path::Path;

use relay_ui::fonts::FACES;
use skrifa::{FontRef, MetadataProvider};

fn has_glyph(c: char) -> bool {
	FACES.iter().any(|face| {
		FontRef::new(face.data)
			.expect("bundled font parses")
			.charmap()
			.map(c)
			.is_some()
	})
}

#[test]
fn translations_only_use_bundled_glyphs() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../localizations");
	let mut missing = Vec::new();
	for language in fs::read_dir(&root).expect("localizations folder") {
		let language = language.expect("entry").path();
		for file in fs::read_dir(&language).expect("language folder") {
			let file = file.expect("entry").path();
			let text = fs::read_to_string(&file).expect("catalog reads");
			for c in text
				.chars()
				.filter(|c| !c.is_whitespace() && !c.is_control())
			{
				if !has_glyph(c) {
					missing.push(format!(
						"{} U+{:04X} {c}",
						file.strip_prefix(&root).unwrap().display(),
						c as u32
					));
				}
			}
		}
	}
	missing.sort();
	missing.dedup();
	assert!(
		missing.is_empty(),
		"characters without a glyph:\n{}",
		missing.join("\n")
	);
}
