//! Nothing draws past the left or right edge of a phone-sized viewport, on
//! every compact page, in the languages with the longest text.

mod phone;

use phone::{LANGUAGES, Phone, SIZES};
use relay_ui::layout::Page;

#[tokio::test]
async fn nothing_overflows_horizontally() {
	let mut bad = Vec::new();
	for language in LANGUAGES {
		for (w, h) in SIZES {
			let mut phone = Phone::new(w, h, language);
			for page in Page::ALL {
				let drawn = phone.page(page);
				let screen = phone.screen().expand(1.0);
				for r in drawn.widgets.iter().chain(&drawn.painted) {
					if r.min.x < screen.min.x || r.max.x > screen.max.x {
						bad.push(format!("{language:?} {w}x{h} {page:?}: {r:?}"));
					}
				}
			}
		}
	}
	bad.dedup();
	assert!(bad.is_empty(), "{} overflows, first: {:#?}", bad.len(), &bad[..bad.len().min(12)]);
}
