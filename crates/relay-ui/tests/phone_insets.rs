//! Content stays out of the system bars: below the status bar and cutout,
//! above the gesture bar, inside the side insets, and above the soft keyboard.

mod phone;

use std::sync::atomic::Ordering;

use phone::{LANGUAGES, Phone, SIZES};
use relay_ui::Insets;
use relay_ui::layout::Page;

const INSETS: Insets = Insets {
	top: 28.0,
	bottom: 20.0,
	left: 12.0,
	right: 12.0,
};

#[tokio::test]
async fn content_respects_the_insets() {
	for language in LANGUAGES {
		for (w, h) in SIZES {
			let mut phone = Phone::new(w, h, language);
			*phone.shell.insets.lock().unwrap() = INSETS;
			for page in Page::ALL {
				let drawn = phone.page(page);
				let ctx = format!("{language:?} {w}x{h} {page:?}");
				for r in drawn.visible.iter().filter(|r| r.width() > 12.0) {
					assert!(
						r.min.y >= INSETS.top - 0.5,
						"{ctx}: under the status bar {r:?}"
					);
					assert!(
						r.max.y <= h - INSETS.bottom + 0.5,
						"{ctx}: over the gesture bar {r:?}"
					);
					assert!(r.min.x >= INSETS.left - 0.5, "{ctx}: left inset {r:?}");
					assert!(
						r.max.x <= w - INSETS.right + 0.5,
						"{ctx}: right inset {r:?}"
					);
				}
				for (r, _) in &drawn.texts {
					assert!(
						r.min.y >= INSETS.top - 0.5 || r.max.y <= 0.0,
						"{ctx}: text in the status bar {r:?}"
					);
					assert!(
						r.max.y <= h - INSETS.bottom + 0.5,
						"{ctx}: text in the gesture bar {r:?}"
					);
				}
			}
		}
	}
}

#[tokio::test]
async fn the_keyboard_pushes_content_up_and_hides_the_tabs() {
	let mut phone = Phone::new(360.0, 800.0, relay_ui::i18n::Language::En);
	*phone.shell.insets.lock().unwrap() = INSETS;
	let before = phone.settle();
	assert_eq!(phone.tabs(&before).len(), Page::ALL.len());
	phone
		.shell
		.keyboard
		.store(300.0_f32.to_bits(), Ordering::SeqCst);
	let drawn = phone.settle();
	assert!(
		phone.tabs(&drawn).is_empty(),
		"tabs stay under the keyboard"
	);
	for r in drawn.visible.iter().filter(|r| r.width() > 12.0) {
		assert!(r.max.y <= 800.0 - 300.0 + 0.5, "under the keyboard {r:?}");
	}
}
