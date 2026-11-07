//! The Contacts card: every trusted contact is listed whether or not it is online, the star and
//! the menu sit at the top right, details sit in a View details accordion that starts closed,
//! not in the menu, and Call is a full-width button with room above it. Fits 360x800, 320x640 and a
//! desktop width in every language, with touch targets of at least 44 points on a phone.

// The turn lock is held on purpose across awaits: the active language is process-wide.
#![allow(clippy::await_holding_lock)]

mod common;
mod phone;

use std::sync::Arc;

use common::FakeOps;
use phone::{Drawn, Phone};
use relay_ui::engine::Command;
use relay_ui::i18n::Language;
use relay_ui::icons::Icon;
use relay_ui::layout::Page;
use relay_ui::model::{AppState, Peer, Trust};

/// The active language is process-wide: tests that draw take turns.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turn() -> std::sync::MutexGuard<'static, ()> {
	TURN.lock().unwrap_or_else(|e| e.into_inner())
}

const SIZES: [(bool, f32, f32); 3] = [
	(false, 360.0, 800.0),
	(false, 320.0, 640.0),
	(true, 720.0, 800.0),
];

fn peer(i: u8, name: &str, reachable: bool) -> Peer {
	let mut peer = phone::busy_state().peers[0].clone();
	peer.peer_id = hex::encode([i + 10; 32]);
	peer.display_name = name.to_owned();
	peer.trust = Trust::Verified;
	peer.reachable = reachable;
	peer.favorite = false;
	peer.last_seen_unix_ms = if reachable { 0 } else { 1_793_870_000_000 };
	peer
}

fn state() -> AppState {
	let mut state = phone::busy_state();
	state.calls.clear();
	state.peers = vec![peer(0, "Ada", true), peer(1, "Offline Olga", false)];
	state
}

fn rig(desktop: bool, w: f32, h: f32, language: Language) -> Phone {
	let phone = if desktop {
		Phone::desktop(w, h, language)
	} else {
		Phone::new(w, h, language)
	};
	let state = state();
	phone.show_state(&state);
	phone.actions.hub.update(|s| *s = state.clone());
	phone
}

fn rect_of(drawn: &Drawn, text: &str) -> egui::Rect {
	drawn
		.named
		.iter()
		.find(|(t, _)| t == text)
		.unwrap_or_else(|| panic!("no {text:?} in {:?}", drawn.labels))
		.1
}

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

fn pressable(drawn: &Drawn, r: egui::Rect) -> bool {
	drawn.tappable.iter().any(|t| t.contains(r.center()))
}

#[tokio::test]
async fn an_offline_contact_is_listed_with_its_state_and_a_greyed_call() {
	let _turn = turn();
	let mut phone = rig(false, 360.0, 1400.0, Language::En);
	let drawn = phone.page(Page::Contacts);
	assert!(
		has(&drawn, "Ada") && has(&drawn, "Offline Olga"),
		"{:?}",
		drawn.labels
	);
	assert!(
		drawn.labels.iter().any(|l| l.starts_with("Offline ")),
		"the state is named: {:?}",
		drawn.labels
	);
	assert!(
		drawn.labels.iter().any(|l| l.starts_with("Last seen")),
		"when it was last seen: {:?}",
		drawn.labels
	);
	assert!(
		!has(&drawn, "This device is offline"),
		"no explanation under Call"
	);
	let calls: Vec<egui::Rect> = drawn
		.named
		.iter()
		.filter(|(t, _)| t == "Call")
		.map(|(_, r)| *r)
		.collect();
	assert_eq!(calls.len(), 2, "{:?}", drawn.labels);
	assert!(
		pressable(&drawn, calls[0]),
		"the online contact can be called"
	);
	assert!(!pressable(&drawn, calls[1]), "the offline one cannot");
}

#[tokio::test]
async fn star_and_menu_are_at_the_top_right_and_call_spans_the_card() {
	let _turn = turn();
	for (desktop, w, h) in SIZES {
		let mut phone = rig(desktop, w, h, Language::En);
		let drawn = phone.page(Page::Calls);
		let drawn = if drawn.labels.iter().any(|l| l == "Offline Olga") {
			drawn
		} else {
			phone.page(Page::Contacts)
		};
		let name = rect_of(&drawn, "Ada");
		let star = phone::icon_rects(&drawn, Icon::Star)[0];
		let more = phone::icon_rects(&drawn, Icon::More)[0];
		assert!(
			star.center().y < name.max.y + 20.0,
			"{w}: the star is in the head row"
		);
		assert!(more.center().y < name.max.y + 20.0, "{w}: so is the menu");
		assert!(
			more.center().x > star.center().x,
			"{w}: menu is the right-most"
		);
		assert!(
			more.max.x > phone.size.x * 0.85 || desktop,
			"{w}: at the right edge"
		);
		let call = rect_of(&drawn, "Call");
		let button = drawn
			.tappable
			.iter()
			.find(|t| t.contains(call.center()))
			.expect("the Call button");
		assert!(
			button.width() > star.max.x.max(more.max.x) * 0.7 - 40.0,
			"{w}: full width"
		);
		assert!(button.min.y - name.max.y >= 20.0, "{w}: room above Call");
	}
}

#[tokio::test]
async fn view_details_is_a_closed_accordion_and_not_in_the_menu() {
	let _turn = turn();
	let mut phone = rig(false, 360.0, 1400.0, Language::En);
	let closed = phone.page(Page::Contacts);
	assert!(has(&closed, "View details"), "{:?}", closed.labels);
	assert!(!closed.labels.iter().any(|l| l.starts_with("Key ")));
	phone.click_icon_nth(Icon::More, 0);
	let menu = phone.settle();
	assert!(has(&menu, "Forget this device") && has(&menu, "Block device"));
	assert_eq!(
		menu.labels.iter().filter(|l| *l == "View details").count(),
		closed
			.labels
			.iter()
			.filter(|l| *l == "View details")
			.count(),
		"the menu adds no View details entry"
	);
	phone.click_label("View details");
	let open = phone.settle();
	assert!(
		open.labels.iter().any(|l| l.starts_with("Key ")),
		"{:?}",
		open.labels
	);
}

#[tokio::test]
async fn call_on_an_online_contact_places_the_call() {
	let _turn = turn();
	let mut phone = rig(false, 360.0, 1400.0, Language::En);
	let ops = Arc::new(FakeOps::default());
	phone.slot.set(ops.clone());
	phone.page(Page::Contacts);
	phone.click_label("Call");
	for _ in 0..100 {
		if !ops.recorded().is_empty() {
			break;
		}
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;
	}
	assert!(matches!(
		ops.recorded().first(),
		Some(Command::StartCall { .. })
	));
}

#[tokio::test]
async fn the_card_fits_in_every_language_at_every_size() {
	let _turn = turn();
	let mut bad = Vec::new();
	for language in Language::ALL {
		for (desktop, w, h) in SIZES {
			let mut phone = rig(desktop, w, h, language);
			let drawn = phone.page(Page::Contacts);
			let screen = phone.screen().expand(1.0);
			for r in drawn.widgets.iter().chain(&drawn.painted) {
				if r.min.x < screen.min.x || r.max.x > screen.max.x {
					bad.push(format!("{language:?} {w}x{h}: overflow {r:?}"));
				}
			}
			if !desktop {
				for r in drawn.tappable.iter().filter(|r| r.width() > 12.0) {
					if r.height() < 43.5 || r.width() < 43.5 {
						bad.push(format!("{language:?} {w}x{h}: small target {:?}", r.size()));
					}
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
