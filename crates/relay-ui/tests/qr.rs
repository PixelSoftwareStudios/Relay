use relay_ui::qr::{modules, render, render_text};

const STRING: &str =
	"relay://192.168.1.20:7000#abababababababababababababababababababababababababababababababab";

#[test]
fn a_connection_string_encodes_to_a_square_grid() {
	let m = modules(STRING).unwrap();
	assert!(
		m.width >= 21 && (m.width - 21).is_multiple_of(4),
		"QR widths are 21 + 4n"
	);
	assert_eq!(m.dark.len(), m.width * m.width);
	assert!(m.dark.iter().any(|d| *d) && m.dark.iter().any(|d| !*d));
	assert_eq!(modules(STRING), modules(STRING), "deterministic");
}

#[test]
fn rendering_adds_a_four_module_white_quiet_zone() {
	let m = modules(STRING).unwrap();
	let img = render(&m, 3);
	let side = (m.width + 8) * 3;
	assert_eq!(img.size, [side, side]);
	assert_eq!(img.rgba.len(), side * side * 4);
	// Corners lie in the quiet zone and the finder pattern starts at (4,4).
	assert_eq!(&img.rgba[0..4], &[255, 255, 255, 255]);
	let finder = (4 * 3 * side + 4 * 3) * 4;
	assert_eq!(&img.rgba[finder..finder + 4], &[0, 0, 0, 255]);
}

#[test]
fn oversized_text_does_not_encode() {
	assert!(render_text(&"x".repeat(5000), 2).is_none());
}

#[test]
fn a_zero_scale_is_treated_as_one() {
	let m = modules("relay://1.2.3.4:5#ab").unwrap();
	assert_eq!(render(&m, 0).size[0], m.width + 8);
}
