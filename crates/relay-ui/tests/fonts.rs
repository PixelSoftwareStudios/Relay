//! The bundled fonts parse and cover every UI language.

use relay_ui::fonts::{self, FACES};
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

#[test]
fn noto_sans_leads_and_cjk_follows() {
	let defs = fonts::definitions();
	let proportional = &defs.families[&egui::FontFamily::Proportional];
	let names: Vec<&str> = FACES.iter().map(|f| f.name).collect();
	assert_eq!(proportional[..names.len()], names[..]);
	let monospace = &defs.families[&egui::FontFamily::Monospace];
	let stock = egui::FontDefinitions::default().families[&egui::FontFamily::Monospace].clone();
	assert_eq!(
		monospace[..stock.len()],
		stock[..],
		"egui monospace stays first"
	);
	assert!(names.iter().all(|n| monospace.iter().any(|m| m == n)));
}

#[test]
fn install_accepts_the_definitions() {
	let ctx = egui::Context::default();
	fonts::install(&ctx);
	let mut out = ctx.run_ui(Default::default(), |ui| {
		ui.label("Relay 設定 설정 सेटिंग्स Настройки");
	});
	out.textures_delta.clear();
}
