//! The five bottom tabs fit side by side and no label breaks mid-word, in the
//! languages with the longest names.

mod phone;

use phone::{LANGUAGES, Phone, SIZES};
use relay_ui::layout::Page;

#[tokio::test]
async fn tabs_fit_and_labels_stay_on_one_line() {
	for language in LANGUAGES {
		for (w, h) in SIZES {
			let mut phone = Phone::new(w, h, language);
			let drawn = phone.settle();
			let tabs = phone.tabs(&drawn);
			let ctx = format!("{language:?} {w}x{h}");
			assert_eq!(tabs.len(), Page::ALL.len(), "{ctx}: {tabs:?}");
			for pair in tabs.windows(2) {
				assert!(pair[0].max.x <= pair[1].min.x + 0.5, "{ctx}: tabs overlap");
			}
			for tab in &tabs {
				assert!(
					tab.min.x >= 0.0 && tab.max.x <= w + 0.5,
					"{ctx}: tab off screen {tab:?}"
				);
				for (rect, rows) in &drawn.texts {
					if tab.contains_rect(*rect) {
						assert_eq!(*rows, 1, "{ctx}: a tab label wrapped");
						assert!(rect.width() <= tab.width(), "{ctx}: label wider than tab");
					}
				}
			}
		}
	}
}
