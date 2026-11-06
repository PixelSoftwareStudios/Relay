//! The open chat fits the window and its controls are touch-sized, in the
//! languages with the longest text. Kept apart from the English tests because
//! the active language is process-wide.

mod chatkit;
mod phone;

use chatkit::{ada_call, live_messages, state};
use phone::{LANGUAGES, Phone};
use relay_ui::i18n::Language;
use relay_ui::layout::MIN_TOUCH_TARGET;

fn desktop(language: Language, ringing: bool) -> Phone {
	let phone = Phone::desktop(720.0, 800.0, language);
	phone.show_state(&state(ringing));
	live_messages(&phone);
	phone
}

fn handset(language: Language, ringing: bool) -> Phone {
	let phone = Phone::new(360.0, 800.0, language);
	phone.show_state(&state(ringing));
	live_messages(&phone);
	phone
}

#[tokio::test]
async fn the_open_chat_stays_inside_the_window_in_every_language() {
	let mut bad = Vec::new();
	for language in LANGUAGES {
		for mut phone in [desktop(language, true), handset(language, true)] {
			phone.app.open_chat(&ada_call());
			let drawn = phone.settle();
			let screen = phone.screen().expand(1.0);
			for r in drawn.widgets.iter().chain(&drawn.painted) {
				if r.min.x < screen.min.x || r.max.x > screen.max.x {
					bad.push(format!("{language:?} {:?}: {r:?}", phone.size));
				}
			}
		}
	}
	bad.dedup();
	assert!(
		bad.is_empty(),
		"{} overflows: {:#?}",
		bad.len(),
		&bad[..bad.len().min(10)]
	);
}

#[tokio::test]
async fn the_phone_chat_has_touch_sized_controls_at_the_smallest_widths() {
	let mut bad = Vec::new();
	for language in LANGUAGES {
		for (w, h) in [(360.0, 800.0), (320.0, 640.0)] {
			let mut phone = Phone::new(w, h, language);
			phone.show_state(&state(true));
			live_messages(&phone);
			phone.app.open_chat(&ada_call());
			let drawn = phone.settle();
			let screen = phone.screen().expand(1.0);
			for r in drawn.widgets.iter().chain(&drawn.painted) {
				if r.min.x < screen.min.x || r.max.x > screen.max.x {
					bad.push(format!("overflow {language:?} {w}x{h}: {r:?}"));
				}
			}
			for r in &drawn.tappable {
				if r.width() > 12.0 && r.height() < MIN_TOUCH_TARGET - 0.5 {
					bad.push(format!("small {language:?} {w}x{h}: {r:?}"));
				}
			}
		}
	}
	bad.sort();
	bad.dedup();
	assert!(
		bad.is_empty(),
		"{} problems: {:#?}",
		bad.len(),
		&bad[..bad.len().min(15)]
	);
}

#[tokio::test]
async fn history_rows_and_their_chat_fit_a_phone_with_touch_sized_controls() {
	let mut bad = Vec::new();
	for language in LANGUAGES {
		for open in [false, true] {
			let mut phone = Phone::new(360.0, 2400.0, language);
			phone.show_state(&chatkit::history_state());
			live_messages(&phone);
			if open {
				phone.app.open_chat(&chatkit::alan_call());
			}
			let drawn = phone.settle();
			let screen = phone.screen().expand(1.0);
			for r in drawn.widgets.iter().chain(&drawn.painted) {
				if r.min.x < screen.min.x || r.max.x > screen.max.x {
					bad.push(format!("overflow {language:?} open={open}: {r:?}"));
				}
			}
			for r in &drawn.tappable {
				if r.width() > 12.0 && r.height() < MIN_TOUCH_TARGET - 0.5 {
					bad.push(format!("small {language:?} open={open}: {r:?}"));
				}
			}
		}
	}
	bad.sort();
	bad.dedup();
	assert!(
		bad.is_empty(),
		"{} problems: {:#?}",
		bad.len(),
		&bad[..bad.len().min(15)]
	);
}
