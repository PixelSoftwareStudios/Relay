//! The bundled Devanagari face shapes correctly with harfrust, the shaper egui
//! 0.37 uses for all text layout: conjuncts form, matras reorder, reph moves.

use harfrust::{ShaperData, UnicodeBuffer};
use relay_ui::fonts::NOTO_SANS_DEVANAGARI;
use skrifa::{FontRef, MetadataProvider};

/// One shaped glyph: id, cluster (byte offset) and advance in font units.
type Shaped = (u32, u32, i32);

fn font() -> FontRef<'static> {
	FontRef::new(NOTO_SANS_DEVANAGARI.data).expect("bundled font parses")
}

fn shape(text: &str) -> Vec<Shaped> {
	let font = font();
	let data = ShaperData::new(&font);
	let shaper = data.shaper(&font).build();
	let mut buffer = UnicodeBuffer::new();
	buffer.push_str(text);
	buffer.guess_segment_properties();
	let out = shaper.shape(buffer, harfrust::ShapeOptions::new());
	out.glyph_infos()
		.iter()
		.zip(out.glyph_positions())
		.map(|(info, pos)| (info.glyph_id, info.cluster, pos.x_advance))
		.collect()
}

/// Naive logical-order mapping: one glyph per code point, no GSUB/GPOS.
fn naive(text: &str) -> Vec<u32> {
	let charmap = font().charmap();
	text.chars()
		.map(|c| charmap.map(c).expect("glyph for code point").to_u32())
		.collect()
}

fn gids(shaped: &[Shaped]) -> Vec<u32> {
	shaped.iter().map(|g| g.0).collect()
}

fn width(shaped: &[Shaped]) -> i32 {
	shaped.iter().map(|g| g.2).sum()
}

#[test]
fn conjunct_kssa_collapses_to_fewer_glyphs() {
	let text = "क्षत्रिय";
	let shaped = shape(text);
	assert_ne!(gids(&shaped), naive(text));
	assert!(
		shaped.len() < text.chars().count(),
		"{} glyphs for {} code points",
		shaped.len(),
		text.chars().count()
	);
}

#[test]
fn hindi_word_forms_a_conjunct() {
	let text = "हिन्दी";
	let shaped = shape(text);
	assert!(shaped.len() < text.chars().count());
	assert!(
		width(&shaped) < width_naive(text),
		"conjunct is narrower than halant plus letter"
	);
}

fn width_naive(text: &str) -> i32 {
	let font = font();
	let charmap = font.charmap();
	let metrics = font.glyph_metrics(
		skrifa::instance::Size::unscaled(),
		skrifa::instance::LocationRef::default(),
	);
	text.chars()
		.map(|c| metrics.advance_width(charmap.map(c).unwrap()).unwrap() as i32)
		.sum()
}

#[test]
fn short_i_matra_moves_before_its_consonant() {
	let text = "कि";
	let naive = naive(text);
	let shaped = shape(text);
	assert_eq!(shaped.len(), 2);
	// Logical order is KA, I-MATRA; visual order draws the matra first.
	// The font may swap in a width-adjusted matra variant, so only the
	// consonant's position and the matra's difference from it are fixed.
	assert_ne!(shaped[0].0, naive[0], "matra glyph leads");
	assert_eq!(shaped[1].0, naive[0], "consonant follows");
}

#[test]
fn reph_moves_after_the_consonant_it_sits_on() {
	let text = "कर्म";
	let naive = naive(text);
	let shaped = shape(text);
	assert_ne!(gids(&shaped), naive);
	// RA + virama before MA becomes a mark drawn above, after KA in visual order.
	assert!(shaped.len() < text.chars().count());
	assert_eq!(shaped[0].0, naive[0], "KA stays first");
}

#[test]
fn width_is_positive_and_stable() {
	for text in ["क्षत्रिय", "हिन्दी", "कि", "कर्म", "नमस्ते दुनिया"]
	{
		let a = width(&shape(text));
		let b = width(&shape(text));
		assert!(a > 0, "{text}");
		assert_eq!(a, b, "{text}");
	}
}
