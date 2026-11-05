use relay_desktop::emitter::TrayState;
use relay_desktop::imaging::decode_for_texture;
use relay_desktop::inbox::Inbox;
use relay_desktop::tray::glyph;

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

#[test]
fn the_avatar_inbox_is_bounded() {
	let inbox = Inbox::default();
	for i in 0..500 {
		inbox.push_avatar(format!("{i}"), None);
	}
	let drained = inbox.take_avatars();
	assert_eq!(drained.len(), 32);
	assert_eq!(drained.last().unwrap().0, "499", "newest results are kept");
	assert!(inbox.take_avatars().is_empty());
}
