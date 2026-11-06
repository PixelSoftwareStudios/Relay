//! Idle power on short viewports: a page whose content overflows and scrolls
//! must ask for no repaint, or only the once-a-second call clock.

mod phone;

use std::time::Duration;

use phone::{Phone, busy_state};
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;

const SHORT: [(f32, f32, bool); 3] = [
	(360.0, 400.0, false),
	(320.0, 640.0, false),
	(720.0, 400.0, true),
];

/// The repaint delay egui asks for after one more frame.
fn delay(phone: &mut Phone) -> Duration {
	let input = egui::RawInput {
		screen_rect: Some(phone.screen()),
		time: Some(10.0),
		..Default::default()
	};
	let app = &mut phone.app;
	let mut out = phone.ctx.run_ui(input, |ui| {
		app.begin_frame(ui.ctx());
		app.ui(ui);
	});
	out.textures_delta.clear();
	out.viewport_output
		.get(&egui::ViewportId::ROOT)
		.map_or(Duration::MAX, |v| v.repaint_delay)
}

fn settled(phone: &mut Phone) -> Duration {
	for _ in 0..20 {
		phone.frame_with(Vec::new());
	}
	// Real time passes between frames: animations only end when time moves.
	for i in 0..120 {
		let input = egui::RawInput {
			screen_rect: Some(phone.screen()),
			time: Some(1.0 + f64::from(i) * 0.05),
			..Default::default()
		};
		let app = &mut phone.app;
		let mut out = phone.ctx.run_ui(input, |ui| {
			app.begin_frame(ui.ctx());
			app.ui(ui);
		});
		out.textures_delta.clear();
	}
	delay(phone)
}

fn rig(w: f32, h: f32, desktop: bool, connected: bool) -> Phone {
	let phone = if desktop {
		Phone::desktop(w, h, Language::En)
	} else {
		Phone::new(w, h, Language::En)
	};
	let mut state = busy_state();
	if !connected {
		state.calls.clear();
	} else {
		for call in &mut state.calls {
			call.phase = relay_ui::model::Phase::Connected;
		}
	}
	phone.show_state(&state);
	phone
}

fn check(connected: bool) {
	check_with(connected, false);
}

fn check_with(connected: bool, hover: bool) {
	let mut bad = Vec::new();
	for (w, h, desktop) in SHORT {
		let pages = Page::ALL.into_iter().map(|p| (p, None)).chain(
			SettingsCategory::ALL
				.into_iter()
				.map(|c| (Page::Settings, Some(c))),
		);
		for (page, category) in pages {
			let mut phone = rig(w, h, desktop, connected);
			phone.app.set_page(page);
			if page == Page::Settings {
				phone.app.open_settings(category);
			}
			if hover {
				let at = egui::pos2(w * 0.7, h * 0.5);
				phone.frame_with(vec![egui::Event::PointerMoved(at)]);
			}
			let d = settled(&mut phone);
			let ok = if connected {
				d >= Duration::from_millis(900)
			} else {
				d >= Duration::from_secs(60)
			};
			if !ok {
				bad.push(format!("{w}x{h} {page:?} {category:?}: {d:?}"));
			}
		}
	}
	assert!(bad.is_empty(), "repaints too often:\n{}", bad.join("\n"));
}

#[tokio::test]
async fn short_pages_without_a_call_never_repaint() {
	check(false);
}

#[tokio::test]
async fn short_pages_with_a_call_repaint_once_a_second_at_most() {
	check(true);
}

#[tokio::test]
async fn short_pages_under_a_resting_pointer_stay_calm() {
	check_with(false, true);
}
