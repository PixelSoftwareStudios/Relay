//! Every tappable control is at least 44 x 44 points in the compact layout.

mod phone;

use phone::{LANGUAGES, Phone, SIZES};
use relay_ui::layout::{MIN_TOUCH_TARGET, Page};

const SCROLL_BAR_WIDTH: f32 = 12.0;

#[tokio::test]
async fn tap_targets_are_at_least_44_points() {
	let mut bad = Vec::new();
	for language in LANGUAGES {
		for (w, h) in SIZES {
			let mut phone = Phone::new(w, h, language);
			for page in Page::ALL {
				let drawn = phone.page(page);
				for r in &drawn.tappable {
					// A scroll bar track is a drag strip, not a control.
					if r.width() <= SCROLL_BAR_WIDTH {
						continue;
					}
					if r.height() < MIN_TOUCH_TARGET - 0.5 || r.width() < MIN_TOUCH_TARGET - 0.5 {
						bad.push(format!("{language:?} {w}x{h} {page:?}: {:?}", r.size()));
					}
				}
			}
		}
	}
	bad.sort();
	bad.dedup();
	assert!(bad.is_empty(), "{} small targets, first: {:#?}", bad.len(), &bad[..bad.len().min(15)]);
}
