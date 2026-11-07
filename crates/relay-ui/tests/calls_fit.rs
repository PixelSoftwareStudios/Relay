//! The Calls page and the Settings categories of this area fit the screen: nothing runs past an
//! edge and, with touch rules, every control is at least 44 points. Phone widths 360 and 320 and
//! a desktop width, in English, German and Russian.

mod phone;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use egui::{Event, Key, Modifiers};
use phone::stats::Ops;
use phone::{Drawn, Phone};
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::{AppState, Mode};

/// 2026-11-05 12:00:00 UTC.
const NOW: u64 = 1_793_880_000_000;
const SIZES: [(bool, f32, f32); 3] = [
	(false, 360.0, 800.0),
	(false, 320.0, 640.0),
	(true, 720.0, 800.0),
];
const LANGS: [Language; 3] = [Language::En, Language::De, Language::Ru];

/// A long name must be cut, never widen its tile.
fn named(state: &mut AppState, long: bool) {
	for (i, peer) in state.peers.iter_mut().enumerate() {
		if long {
			peer.display_name = format!("Aleksandra Konstantinopolskaya-Wolfeschlegelstein {i}");
		}
	}
}

fn states() -> Vec<(&'static str, AppState)> {
	let busy = phone::busy_state();
	let mut grid = busy.clone();
	grid.calls.clear();
	named(&mut grid, true);
	let mut empty = busy.clone();
	empty.calls.clear();
	for p in &mut empty.peers {
		p.favorite = false;
	}
	let mut nobody = busy.clone();
	nobody.calls.clear();
	nobody.peers.clear();
	let mut call = busy.clone();
	call.calls.retain(|c| c.call_id == "02".repeat(16));
	call.calls[0].connected_at_unix_ms = NOW - 3_700_000;
	call.calls[0].transmit.mode = Mode::PushToTalk;
	let mut muted = call.clone();
	muted.calls[0].transmit.mode = Mode::Muted;
	vec![
		("grid", grid),
		("no favorites", empty),
		("no contacts", nobody),
		("incoming and call", busy),
		("push to talk", call),
		("muted", muted),
	]
}

fn check(tag: &str, drawn: &Drawn, phone: &Phone, touch: bool, bad: &mut Vec<String>) {
	let screen = phone.screen().expand(1.0);
	for r in drawn.widgets.iter().chain(&drawn.painted) {
		if r.min.x < screen.min.x || r.max.x > screen.max.x {
			bad.push(format!("{tag}: overflow {r:?}"));
		}
	}
	if touch {
		for r in drawn.tappable.iter().filter(|r| r.width() > 12.0) {
			if r.height() < 43.5 || r.width() < 43.5 {
				bad.push(format!("{tag}: small target {:?}", r.size()));
			}
		}
	}
}

#[tokio::test]
async fn the_calls_page_fits_with_every_content_in_every_language() {
	let mut bad = Vec::new();
	for language in LANGS {
		for (desktop, w, h) in SIZES {
			for (name, state) in states() {
				let mut phone = if desktop {
					Phone::desktop(w, h * 3.0, language)
				} else {
					Phone::new(w, h * 3.0, language)
				};
				phone.shell.now_ms.store(NOW, Ordering::SeqCst);
				phone.show_state(&state);
				let drawn = phone.page(Page::Calls);
				check(
					&format!("{language:?} {w}x{h} {name}"),
					&drawn,
					&phone,
					!desktop,
					&mut bad,
				);
			}
		}
	}
	bad.sort();
	bad.dedup();
	assert!(
		bad.is_empty(),
		"{} problems: {:#?}",
		bad.len(),
		&bad[..bad.len().min(8)]
	);
}

#[tokio::test]
async fn the_connection_details_fit_with_data_in_every_language() {
	let mut bad = Vec::new();
	for language in LANGS {
		for (desktop, w, h) in SIZES {
			let mut phone = if desktop {
				Phone::desktop(w, h * 3.0, language)
			} else {
				Phone::new(w, h * 3.0, language)
			};
			phone.shell.now_ms.store(NOW, Ordering::SeqCst);
			let mut state = states().remove(4).1;
			state.link = relay_ui::model::Link::Connected;
			phone.show_state(&state);
			let ops = Arc::new(Ops::default());
			phone.slot.set(ops.clone());
			phone.settle();
			phone.frame_with(vec![Event::Key {
				key: Key::D,
				physical_key: None,
				pressed: true,
				repeat: false,
				modifiers: Modifiers::COMMAND | Modifiers::SHIFT,
			}]);
			let call = "02".repeat(16);
			for _ in 0..200 {
				if phone.actions.shared.stats.latest(&call).is_some() {
					break;
				}
				tokio::time::sleep(Duration::from_millis(10)).await;
			}
			phone.settle();
			phone.click_label(relay_ui::i18n::t("action-details"));
			let drawn = phone.settle();
			assert!(
				drawn
					.labels
					.iter()
					.any(|l| l == relay_ui::i18n::t("details-title")),
				"{language:?}: the panel is open"
			);
			check(
				&format!("{language:?} {w}x{h} details"),
				&drawn,
				&phone,
				!desktop,
				&mut bad,
			);
		}
	}
	bad.sort();
	bad.dedup();
	assert!(
		bad.is_empty(),
		"{} problems: {:#?}",
		bad.len(),
		&bad[..bad.len().min(8)]
	);
}

#[tokio::test]
async fn the_settings_of_this_area_fit_in_every_language() {
	let mut bad = Vec::new();
	for language in LANGS {
		for (desktop, w, h) in SIZES {
			for category in [
				SettingsCategory::Profile,
				SettingsCategory::PushToTalk,
				SettingsCategory::Calls,
			] {
				{
					let mut phone = if desktop {
						Phone::desktop(w, h * 2.0, language)
					} else {
						Phone::new(w, h * 2.0, language)
					};
					let state = phone::busy_state();
					phone.show_state(&state);
					phone.app.open_settings(Some(category));
					let drawn = phone.settle();
					check(
						&format!("{language:?} {w}x{h} {category:?}"),
						&drawn,
						&phone,
						!desktop,
						&mut bad,
					);
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
		&bad[..bad.len().min(8)]
	);
}
