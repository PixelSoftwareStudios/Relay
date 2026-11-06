//! egui's own layout applies the shaping: a conjunct is narrower in a galley
//! than the sum of its code points drawn one by one.

use egui::{Color32, FontId};
use relay_ui::fonts;
use relay_ui::i18n::Language;

fn width(ctx: &egui::Context, text: &str) -> f32 {
	let mut width = 0.0;
	let mut out = ctx.run_ui(Default::default(), |ui| {
		let galley = ui.painter().layout_no_wrap(
			text.to_owned(),
			FontId::proportional(20.0),
			Color32::WHITE,
		);
		width = galley.size().x;
	});
	out.textures_delta.clear();
	width
}

#[test]
fn galley_width_reflects_conjuncts_and_stays_stable() {
	let ctx = egui::Context::default();
	fonts::install(&ctx, Language::Hi);
	// First frame uploads the fonts.
	width(&ctx, "क");
	let text = "क्षत्रिय";
	let whole = width(&ctx, text);
	let parts: f32 = text.chars().map(|c| width(&ctx, &c.to_string())).sum();
	assert!(whole > 0.0);
	assert!(whole < parts, "shaped {whole} vs per-code-point {parts}");
	assert_eq!(whole, width(&ctx, text));
}
