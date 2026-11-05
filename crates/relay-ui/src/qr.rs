//! QR rendering of a connection string. Generated only while the connection
//! panel is open; the result is plain pixels, so it tests without a window.

use qrcode::{Color, EcLevel, QrCode};

use crate::imaging::Decoded;

/// Quiet-zone width in modules required around a QR code.
const QUIET: usize = 4;

/// A QR code as dark/light modules, without the quiet zone.
#[derive(Debug, PartialEq, Eq)]
pub struct Modules {
	pub width: usize,
	pub dark: Vec<bool>,
}

/// Encode text. `None` when it does not fit the largest QR version.
pub fn modules(text: &str) -> Option<Modules> {
	let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
	Some(Modules {
		width: code.width(),
		dark: code
			.into_colors()
			.into_iter()
			.map(|c| c == Color::Dark)
			.collect(),
	})
}

/// Black on white pixels with a quiet zone, `scale` pixels per module.
pub fn render(modules: &Modules, scale: usize) -> Decoded {
	let scale = scale.max(1);
	let side = (modules.width + 2 * QUIET) * scale;
	let mut rgba = vec![255u8; side * side * 4];
	for (i, dark) in modules.dark.iter().enumerate() {
		if !dark {
			continue;
		}
		let (mx, my) = (i % modules.width + QUIET, i / modules.width + QUIET);
		for y in my * scale..(my + 1) * scale {
			for x in mx * scale..(mx + 1) * scale {
				let at = (y * side + x) * 4;
				rgba[at..at + 3].fill(0);
			}
		}
	}
	Decoded {
		size: [side, side],
		rgba,
	}
}

/// Convenience: encode and render in one step.
pub fn render_text(text: &str, scale: usize) -> Option<Decoded> {
	modules(text).map(|m| render(&m, scale))
}
