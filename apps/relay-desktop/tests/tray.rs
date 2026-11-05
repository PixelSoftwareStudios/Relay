use relay_desktop::emitter::TrayState;
use relay_desktop::tray::glyph;
use relay_ui::imaging::decode_for_texture;

#[test]
fn every_tray_state_has_a_decodable_glyph() {
	for state in [TrayState::Idle, TrayState::InCall, TrayState::Muted] {
		let image = decode_for_texture(glyph(state)).expect("glyph decodes");
		assert!(image.size[0] >= 16 && image.size[0] == image.size[1]);
	}
}

#[test]
fn glyphs_differ_per_state() {
	assert_ne!(glyph(TrayState::Idle), glyph(TrayState::InCall));
	assert_ne!(glyph(TrayState::InCall), glyph(TrayState::Muted));
}
