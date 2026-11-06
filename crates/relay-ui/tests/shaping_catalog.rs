//! Every Devanagari run in the Hindi catalog shapes without `.notdef` glyphs.

use std::fs;
use std::path::Path;

use harfrust::{ShaperData, UnicodeBuffer};
use relay_ui::fonts::NOTO_SANS_DEVANAGARI;
use skrifa::{FontRef, MetadataProvider};

#[test]
fn hindi_catalog_shapes_without_notdef() {
	let font = FontRef::new(NOTO_SANS_DEVANAGARI.data).expect("bundled font parses");
	let charmap = font.charmap();
	let data = ShaperData::new(&font);
	let shaper = data.shaper(&font).build();
	let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../localizations/hi");
	let mut shaped_runs = 0;
	let mut bad = Vec::new();
	for file in fs::read_dir(&dir).expect("hi folder") {
		let text = fs::read_to_string(file.expect("entry").path()).expect("catalog reads");
		// Latin and punctuation come from Noto Sans in the app; shape the
		// rest as maximal runs the Devanagari face covers.
		let mut run = String::new();
		let mut runs = Vec::new();
		for c in text.chars() {
			if !c.is_whitespace() && charmap.map(c).is_some() {
				run.push(c);
			} else if !run.is_empty() {
				runs.push(std::mem::take(&mut run));
			}
		}
		runs.extend((!run.is_empty()).then_some(run));
		for run in runs {
			if !run.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c)) {
				continue;
			}
			let mut buffer = UnicodeBuffer::new();
			buffer.push_str(&run);
			buffer.guess_segment_properties();
			let out = shaper.shape(buffer, harfrust::ShapeOptions::new());
			shaped_runs += 1;
			if out.glyph_infos().iter().any(|g| g.glyph_id == 0) {
				bad.push(run);
			}
		}
	}
	assert!(shaped_runs > 100, "only {shaped_runs} runs found");
	bad.sort();
	bad.dedup();
	assert!(bad.is_empty(), "runs with .notdef:\n{}", bad.join("\n"));
}
