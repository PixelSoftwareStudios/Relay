//! The notices strip on screen: dismissal, the three-line limit, fading,
//! layout at phone and desktop sizes and repaints.

mod phone;

use std::sync::atomic::Ordering;
use std::time::Duration;

use phone::{Drawn, Phone};
use relay_ui::i18n::Language;
use relay_ui::icons::Icon;
use relay_ui::layout::Page;
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

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l.contains(text))
}

fn repaint_delay(phone: &mut Phone) -> Duration {
	let input = egui::RawInput {
		screen_rect: Some(phone.screen()),
		..Default::default()
	};
	let app = &mut phone.app;
	let out = phone.ctx.run_ui(input, |ui| {
		app.begin_frame(ui.ctx());
		app.ui(ui);
	});
	out.viewport_output
		.get(&egui::ViewportId::ROOT)
		.map_or(Duration::MAX, |v| v.repaint_delay)
}

#[tokio::test]
async fn dismissing_hides_a_notice_and_it_stays_hidden_after_a_refresh() {
	let state = with_warnings(1);
	let mut phone = rig(360.0, 800.0, &state);
	assert!(has(&phone.settle(), "Warning number 0"));
	phone.click_icon(Icon::Close);
	assert!(!has(&phone.settle(), "Warning number 0"));
	phone.show_state(&state);
	assert!(
		!has(&phone.settle(), "Warning number 0"),
		"same notice, refreshed"
	);
	let mut again = state.clone();
	again.warnings[0].message = "Warning number 0, again".into();
	phone.show_state(&again);
	assert!(has(&phone.settle(), "again"), "a new occurrence shows");
	phone.show_state(&base());
	assert!(!has(&phone.settle(), "again"), "withdrawn");
}

#[tokio::test]
async fn a_withdrawn_and_raised_again_warning_shows_again() {
	let state = with_warnings(1);
	let mut phone = rig(720.0, 800.0, &state);
	phone.settle();
	phone.click_icon(Icon::Close);
	phone.show_state(&base());
	phone.settle();
	phone.show_state(&state);
	assert!(has(&phone.settle(), "Warning number 0"));
}

#[tokio::test]
async fn at_most_three_show_with_a_plus_n_more_expander_and_dismiss_all() {
	let state = with_warnings(5);
	let mut phone = rig(360.0, 800.0, &state);
	let drawn = phone.settle();
	assert!(has(&drawn, "Warning number 2") && !has(&drawn, "Warning number 3"));
	assert!(has(&drawn, "+2 more") && has(&drawn, "Dismiss all"));
	phone.click_label("+2 more");
	let drawn = phone.settle();
	assert!(has(&drawn, "Warning number 4") && has(&drawn, "Show fewer"));
	phone.click_label("Dismiss all");
	let drawn = phone.settle();
	assert!(!has(&drawn, "Warning number") && !has(&drawn, "Dismiss all"));
}

#[tokio::test]
async fn a_single_notice_offers_no_dismiss_all() {
	let mut phone = rig(360.0, 800.0, &with_warnings(1));
	assert!(!has(&phone.settle(), "Dismiss all"));
}

#[tokio::test]
async fn identical_messages_show_once_with_a_count() {
	let mut state = base();
	state.warnings = vec![
		problem("MEDIA_WORKER_FAILED", "a"),
		problem("MEDIA_WORKER_FAILED", "b"),
	];
	let mut phone = rig(720.0, 800.0, &state);
	let drawn = phone.settle();
	assert_eq!(
		drawn
			.labels
			.iter()
			.filter(|l| l.contains("Audio stopped for a moment"))
			.count(),
		1
	);
	assert!(has(&drawn, "(2 times)"));
}

#[tokio::test]
async fn an_info_notice_fades_after_ten_seconds_unless_hovered_and_warnings_stay() {
	let mut state = with_warnings(1);
	state.notice = Some("Background service stopped.".into());
	let mut phone = rig(720.0, 800.0, &state);
	assert!(has(&phone.settle(), "Background service stopped."));
	phone.shell.now_ms.store(T0 + 9_000, Ordering::SeqCst);
	assert!(has(&phone.settle(), "Background service stopped."));
	phone.shell.now_ms.store(T0 + 10_500, Ordering::SeqCst);
	let drawn = phone.settle();
	assert!(!has(&drawn, "Background service stopped."));
	assert!(has(&drawn, "Warning number 0"), "warnings stay");
}

#[tokio::test]
async fn the_strip_is_calm_until_an_info_notice_counts_down() {
	// Tall, so the page itself does not scroll (a scrolling page repaints).
	let mut phone = rig(360.0, 3000.0, &with_warnings(2));
	phone.settle();
	assert_eq!(
		repaint_delay(&mut phone),
		Duration::MAX,
		"warnings only: idle"
	);
	let mut state = base();
	state.notice = Some("Background service stopped.".into());
	phone.show_state(&state);
	phone.settle();
	let delay = repaint_delay(&mut phone);
	assert!(delay <= Duration::from_secs(10), "{delay:?}");
	phone.shell.now_ms.store(T0 + 20_000, Ordering::SeqCst);
	phone.settle();
	assert_eq!(repaint_delay(&mut phone), Duration::MAX, "faded: idle");
}

#[tokio::test]
async fn settings_shows_no_notices() {
	let mut phone = rig(720.0, 800.0, &with_warnings(2));
	phone.app.open_settings(None);
	assert!(!has(&phone.settle(), "Warning number"));
}
