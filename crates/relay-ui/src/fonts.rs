//! Bundled UI fonts: Noto Sans first, then CJK and Devanagari fallbacks, in an
//! order that depends on the UI language.
//!
//! The subsets live in the root `assets/font` folder (see its README for
//! provenance, licence and the reproducible build). egui picks the first
//! family member that has a glyph, so the order is the fallback order.
//!
//! egui does no complex text shaping, so Devanagari conjuncts and reordering are
//! not applied: Hindi is readable but flagged experimental (see `docs/i18n.md`).

use std::sync::Arc;

use crate::i18n::Language;
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

/// Every bundled face, in the default fallback order.
pub const FACES: [Face; 5] = [
	NOTO_SANS,
	NOTO_SANS_JP,
	NOTO_SANS_KR,
	NOTO_SANS_SC,
	NOTO_SANS_DEVANAGARI,
];

/// The faces in lookup order for `language`. Noto Sans always leads (Latin,
/// Cyrillic, Greek). The CJK faces share Han characters but draw them in
/// different regional forms, so the language's own face comes first: Simplified
/// Chinese for `zh-CN`, Japanese for `ja`, Korean for `ko`. Devanagari has no
/// overlap with them and sits right after Noto Sans for Hindi. Other languages
/// use the default order (Japanese before Chinese), which only matters for
/// Han characters in names typed by peers.
pub fn order(language: Language) -> [Face; 5] {
	let [sans, jp, kr, sc, dev] = FACES;
	match language {
		Language::ZhCn => [sans, sc, jp, kr, dev],
		Language::Ja => [sans, jp, sc, kr, dev],
		Language::Ko => [sans, kr, jp, sc, dev],
		Language::Hi => [sans, dev, jp, kr, sc],
		_ => [sans, jp, kr, sc, dev],
	}
}

/// egui font definitions for `language`: the bundled faces lead the
/// proportional family and trail the monospace family (egui's own monospace
/// face stays first there).
pub fn definitions(language: Language) -> FontDefinitions {
	let mut defs = FontDefinitions::default();
	for face in FACES {
		defs.font_data.insert(
			face.name.to_owned(),
			Arc::new(FontData::from_static(face.data)),
		);
	}
	let faces = order(language);
	let proportional = defs.families.entry(FontFamily::Proportional).or_default();
	for (index, face) in faces.iter().enumerate() {
		proportional.insert(index, face.name.to_owned());
	}
	let monospace = defs.families.entry(FontFamily::Monospace).or_default();
	monospace.extend(faces.iter().map(|face| face.name.to_owned()));
	defs
}

/// Install the bundled fonts for `language` into `ctx`. Called at start-up
/// and again whenever the language changes.
pub fn install(ctx: &Context, language: Language) {
	ctx.set_fonts(definitions(language));
}
