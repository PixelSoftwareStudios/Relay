//! The notices strip fits every width in the longest languages with 44 pt
//! targets on touch layouts. Alone in its file: it switches the language.

mod phone;

use std::sync::atomic::Ordering;

use phone::{Drawn, Phone};
use relay_ui::i18n::Language;
use relay_ui::icons::Icon;
use relay_ui::layout::{MIN_TOUCH_TARGET, Page};
use relay_ui::model::{AppState, Problem};

const T0: u64 = 1_700_000_000_000;

fn problem(code: &str, message: &str) -> Problem {
	Problem {
		code: code.into(),
		message: message.into(),
		retryable: false,
		recovery_action: String::new(),
	}
}

fn base() -> AppState {
	let mut s = phone::busy_state();
	s.calls.clear();
	s
}

fn with_warnings(n: usize) -> AppState {
	let mut s = base();
	s.warnings = (0..n)
		.map(|i| problem("SOMETHING", &format!("Warning number {i}")))
		.collect();
	s
}

fn rig(width: f32, height: f32, state: &AppState) -> Phone {
	let mut phone = if width >= 600.0 {
		Phone::desktop(width, height, Language::En)
	} else {
		Phone::new(width, height, Language::En)
	};
	phone.shell.now_ms.store(T0, Ordering::SeqCst);
	phone.show_state(state);
	phone.app.set_page(Page::Calls);
	phone
}

fn closes(drawn: &Drawn) -> usize {
	phone::icon_rects(drawn, Icon::Close).len()
}

#[tokio::test]
async fn the_strip_fits_with_big_targets_at_every_size() {
	let mut state = with_warnings(5);
	state.warnings[0] = problem(
		"PEER_KEY_CHANGED",
		"`Dantes-Laptop.local` presented key 59F1-AAAA instead of the pinned 1A10-BBBB; the link was refused",
	);
	state.notice = Some("Background service stopped.".into());
	for (w, h) in [(360.0, 800.0), (320.0, 640.0), (720.0, 800.0)] {
		for language in [Language::En, Language::De, Language::Ru] {
			let mut phone = rig(w, h, &state);
			relay_ui::i18n::set_active(language);
			let drawn = phone.settle();
			let screen = phone.screen().expand(1.0);
			for r in drawn.widgets.iter().chain(&drawn.painted) {
				assert!(
					r.min.x >= screen.min.x && r.max.x <= screen.max.x,
					"{w}x{h} {language:?}: {r:?}"
				);
			}
			assert!(closes(&drawn) >= 1, "{w}x{h}");
			if w < 600.0 {
				for r in phone::icon_rects(&drawn, Icon::Close) {
					let hit = drawn.tappable.iter().find(|t| t.contains(r.center()));
					let hit = hit.expect("close button is tappable");
					assert!(
						hit.width() >= MIN_TOUCH_TARGET - 0.5
							&& hit.height() >= MIN_TOUCH_TARGET - 0.5
					);
				}
				for t in drawn.tappable.iter().filter(|t| t.width() > 12.0) {
					assert!(t.height() >= MIN_TOUCH_TARGET - 0.5, "{w}x{h}: {t:?}");
				}
			}
		}
	}
	relay_ui::i18n::set_active(Language::En);
}
