//! Bundled UI fonts: Noto Sans first, then CJK and Devanagari fallbacks.
//!
//! The subsets live in the root `assets/font` folder (see its README for
//! provenance, licence and the reproducible build). egui picks the first
//! family member that has a glyph, so the order below is the fallback order.

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily};

/// One bundled face: its egui registry name and the font bytes.
#[derive(Clone, Copy, Debug)]
pub struct Face {
	pub name: &'static str,
	pub data: &'static [u8],
}

/// Noto Sans: Latin, Latin Extended, Greek and Cyrillic.
pub const NOTO_SANS: Face = Face {
	name: "NotoSans-Regular",
	data: include_bytes!("../../../assets/font/NotoSans-Regular.ttf"),
};
/// Japanese: JIS X 0208 level 1, Joyo kanji, kana, punctuation.
pub const NOTO_SANS_JP: Face = Face {
	name: "NotoSansJP-Regular",
	data: include_bytes!("../../../assets/font/NotoSansJP-Regular.otf"),
};
/// Korean: KS X 1001 Hangul syllables, Jamo, symbols.
pub const NOTO_SANS_KR: Face = Face {
	name: "NotoSansKR-Regular",
	data: include_bytes!("../../../assets/font/NotoSansKR-Regular.otf"),
};
/// Simplified Chinese: GB2312.
pub const NOTO_SANS_SC: Face = Face {
	name: "NotoSansSC-Regular",
	data: include_bytes!("../../../assets/font/NotoSansSC-Regular.otf"),
};
/// Devanagari (Hindi).
pub const NOTO_SANS_DEVANAGARI: Face = Face {
	name: "NotoSansDevanagari-Regular",
	data: include_bytes!("../../../assets/font/NotoSansDevanagari-Regular.ttf"),
};

/// Every bundled face in fallback order.
pub const FACES: [Face; 5] = [
	NOTO_SANS,
	NOTO_SANS_JP,
	NOTO_SANS_KR,
	NOTO_SANS_SC,
	NOTO_SANS_DEVANAGARI,
];

/// egui font definitions: the bundled faces lead the proportional family and
/// trail the monospace family (egui's own monospace face stays first there).
pub fn definitions() -> FontDefinitions {
	let mut defs = FontDefinitions::default();
	for face in FACES {
		defs.font_data.insert(
			face.name.to_owned(),
			Arc::new(FontData::from_static(face.data)),
		);
	}
	let proportional = defs.families.entry(FontFamily::Proportional).or_default();
	for (index, face) in FACES.iter().enumerate() {
		proportional.insert(index, face.name.to_owned());
	}
	let monospace = defs.families.entry(FontFamily::Monospace).or_default();
	monospace.extend(FACES.iter().map(|face| face.name.to_owned()));
	defs
}

/// Install the bundled fonts into `ctx`. Call once at start-up.
pub fn install(ctx: &Context) {
	ctx.set_fonts(definitions());
}
