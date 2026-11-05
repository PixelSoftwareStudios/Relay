//! The bundled fonts parse and cover every UI language.

use relay_ui::fonts::{self, FACES};
use relay_ui::i18n::Language;
use skrifa::{FontRef, MetadataProvider};

/// Representative strings per UI language: app name, a few UI words and a
/// sentence with the script's typical punctuation. Real translations extend
/// this table.
const SAMPLES: [(&str, &str); 9] = [
	(
		"English",
		"Relay Settings Hang up. “Quoted” — it’s 3 … done · ok",
	),
	(
		"Spanish",
		"Relay Ajustes Colgar. ¿Qué pasó? ¡Señal débil! Niño, camión, ü",
	),
	(
		"German",
		"Relay Einstellungen Auflegen. Größe, Straße, Äpfel, Öl, Übung, „Zitat“",
	),
	(
		"Russian",
		"Relay Настройки Положить трубку. Ёлка, привет, мир! Щука, ъ, ы",
	),
	(
		"Portuguese (Brazil)",
		"Relay Configurações Desligar. Ação, não, você, coração, às, ê, õ",
	),
	(
		"Korean",
		"Relay 설정 전화 끊기 안녕하세요. 한국어 음성 채널 연결됨",
	),
	(
		"Japanese",
		"Relay 設定 通話を終了 こんにちは。音声チャンネル、接続済み。カタカナ",
	),
	(
		"Simplified Chinese",
		"Relay 设置 挂断 你好，语音频道已连接。网络状态：良好",
	),
	(
		"Hindi",
		"Relay सेटिंग्स कॉल समाप्त करें नमस्ते। हिन्दी आवाज़ चैनल जुड़ा हुआ है",
	),
];

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
fn every_face_parses_and_has_glyphs() {
	for face in FACES {
		let font = FontRef::new(face.data).unwrap_or_else(|e| panic!("{}: {e}", face.name));
		assert!(font.charmap().mappings().count() > 100, "{}", face.name);
	}
}

#[test]
fn every_sample_character_has_a_glyph() {
	for (language, text) in SAMPLES {
		let missing: Vec<char> = text
			.chars()
			.filter(|c| !c.is_whitespace() && !has_glyph(*c))
			.collect();
		assert!(missing.is_empty(), "{language}: no glyph for {missing:?}");
	}
}

fn proportional(language: Language) -> Vec<String> {
	fonts::definitions(language).families[&egui::FontFamily::Proportional].clone()
}

#[test]
fn noto_sans_leads_every_language_and_the_cjk_face_follows_the_language() {
	let first_cjk = |language| {
		proportional(language)
			.into_iter()
			.find(|n| n.contains("SC") || n.contains("JP") || n.contains("KR"))
			.unwrap()
	};
	for language in Language::ALL {
		assert_eq!(proportional(language)[0], fonts::NOTO_SANS.name);
		let defs = fonts::definitions(language);
		let monospace = &defs.families[&egui::FontFamily::Monospace];
		let stock = egui::FontDefinitions::default().families[&egui::FontFamily::Monospace].clone();
		assert_eq!(
			monospace[..stock.len()],
			stock[..],
			"egui monospace stays first"
		);
		for face in FACES {
			assert!(monospace.iter().any(|m| m == face.name));
		}
	}
	assert_eq!(first_cjk(Language::ZhCn), fonts::NOTO_SANS_SC.name);
	assert_eq!(first_cjk(Language::Ja), fonts::NOTO_SANS_JP.name);
	assert_eq!(first_cjk(Language::Ko), fonts::NOTO_SANS_KR.name);
	assert_eq!(first_cjk(Language::En), fonts::NOTO_SANS_JP.name);
	assert_eq!(
		proportional(Language::Hi)[1],
		fonts::NOTO_SANS_DEVANAGARI.name
	);
}

#[test]
fn install_accepts_the_definitions_of_every_language() {
	for language in Language::ALL {
		let ctx = egui::Context::default();
		fonts::install(&ctx, language);
		let mut out = ctx.run_ui(Default::default(), |ui| {
			ui.label("Relay 設定 설정 सेटिंग्स Настройки");
		});
		out.textures_delta.clear();
	}
}
