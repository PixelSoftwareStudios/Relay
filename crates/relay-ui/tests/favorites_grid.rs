//! The Favorites section of the Calls page: a grid of up to six tiles, three to a row, a call
//! per tap, a press-and-hold menu, and the empty states with their two buttons.

mod common;
mod phone;

use std::sync::Arc;
use std::time::Duration;

use common::FakeOps;
use egui::{Event, PointerButton};
use phone::{Drawn, Phone};
use relay_ui::engine::Command;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::{AppState, Trust};

const NAMES: [&str; 6] = ["Ann", "Bea", "Cid", "Dee", "Eli", "Flo"];

/// `favorites` of the six people are on the grid; every one is online and trusted.
fn state(favorites: usize, contacts: usize) -> AppState {
	let mut state = phone::busy_state();
	state.calls.clear();
	state.peers = (0..contacts)
		.map(|i| {
			let mut peer = state_peer(i);
			peer.favorite = i < favorites;
			peer.favorite_position = i as u32;
			peer
		})
		.collect();
	state
}

fn state_peer(i: usize) -> relay_ui::model::Peer {
	let mut peer = phone::busy_state().peers[0].clone();
	peer.peer_id = hex::encode([i as u8 + 10; 32]);
	peer.display_name = NAMES[i].to_owned();
	peer.trust = Trust::Verified;
	peer.reachable = true;
	peer
}

fn rig(width: f32, state: &AppState) -> Phone {
	let mut phone = Phone::new(width, 1400.0, Language::En);
	phone.show_state(state);
	// The controller judges requests by the hub's state.
	phone.actions.hub.update(|s| *s = state.clone());
	phone.settle();
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

fn connected(phone: &Phone) -> Arc<FakeOps> {
	let ops = Arc::new(FakeOps::default());
	phone.slot.set(ops.clone());
	ops
}

async fn until(mut done: impl FnMut() -> bool) {
	for _ in 0..100 {
		if done() {
			return;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!("condition not reached in time");
}

fn secondary_click(phone: &mut Phone, pos: egui::Pos2) {
	let button = |pressed| Event::PointerButton {
		pos,
		button: PointerButton::Secondary,
		pressed,
		modifiers: Default::default(),
	};
	phone.frame_with(vec![Event::PointerMoved(pos)]);
	phone.frame_with(vec![button(true)]);
	phone.frame_with(vec![button(false)]);
	phone.settle();
}

#[tokio::test]
async fn zero_to_six_favorites_make_rows_of_three() {
	for count in 1..=6 {
		for width in [360.0, 320.0] {
			let mut phone = rig(width, &state(count, 6));
			let drawn = phone.page(Page::Calls);
			let tiles: Vec<_> = NAMES[..count].iter().map(|n| rect_of(&drawn, n)).collect();
			let mut rows: Vec<f32> = tiles.iter().map(|r| r.center().y.round()).collect();
			rows.dedup();
			assert_eq!(
				rows.len(),
				count.div_ceil(3),
				"{count} favorites at {width}"
			);
			for (i, tile) in tiles.iter().enumerate() {
				assert!(
					tile.min.x >= 0.0 && tile.max.x <= width,
					"{tile:?} at {width}"
				);
				if i % 3 > 0 {
					assert!(tile.min.x > tiles[i - 1].min.x, "left to right");
				}
			}
			for name in &NAMES[count..] {
				assert!(!has(&drawn, name), "{name} is not a favorite");
			}
		}
	}
}

#[tokio::test]
async fn a_tap_calls_and_an_offline_tile_says_why_and_does_nothing() {
	let mut st = state(3, 3);
	st.peers[1].reachable = false;
	let mut phone = rig(360.0, &st);
	let ops = connected(&phone);
	let drawn = phone.page(Page::Calls);
	assert_eq!(drawn.labels.iter().filter(|l| *l == "Offline").count(), 1);
	phone.click_label("Bea");
	tokio::time::sleep(Duration::from_millis(100)).await;
	assert!(ops.recorded().is_empty(), "offline: {:?}", ops.recorded());
	phone.click_label("Cid");
	until(|| !ops.recorded().is_empty()).await;
	assert_eq!(
		ops.recorded(),
		[Command::StartCall {
			peer_id: vec![12; 32]
		}]
	);
}

#[tokio::test]
async fn a_right_click_opens_the_menu_to_remove_or_open_the_contact() {
	let mut phone = rig(720.0, &state(3, 3));
	let ops = connected(&phone);
	let drawn = phone.page(Page::Calls);
	secondary_click(&mut phone, rect_of(&drawn, "Bea").center());
	phone.click_label("Remove from favorites");
	until(|| !ops.recorded().is_empty()).await;
	match ops.recorded().last() {
		Some(Command::SetFavorites { peer_ids }) => {
			assert_eq!(peer_ids, &[vec![10; 32], vec![12; 32]]);
		}
		other => panic!("{other:?}"),
	}
	let drawn = phone.page(Page::Calls);
	secondary_click(&mut phone, rect_of(&drawn, "Ann").center());
	phone.click_label("Open contact");
	assert_eq!(phone.app.page(), Page::Contacts);
}

#[tokio::test]
async fn with_no_favorites_the_empty_state_points_to_contacts_and_nearby() {
	let mut phone = rig(360.0, &state(0, 3));
	let drawn = phone.page(Page::Calls);
	assert!(has(&drawn, "No favorites yet"), "{:?}", drawn.labels);
	assert!(has(
		&drawn,
		"Pin your favorite contacts here for one-tap calling. Star a contact in Contacts."
	));
	assert!(!has(&drawn, "Nobody to call yet") && !has(&drawn, "Online now"));
	phone.click_label("Go to Contacts");
	assert_eq!(phone.app.page(), Page::Contacts);
	phone.app.set_page(Page::Calls);
	phone.click_label("Find nearby users");
	assert_eq!(phone.app.page(), Page::Connect);
}

#[tokio::test]
async fn with_no_contacts_at_all_the_empty_state_points_to_connect() {
	let mut phone = rig(360.0, &state(0, 0));
	let drawn = phone.page(Page::Calls);
	assert!(has(&drawn, "No contacts yet"), "{:?}", drawn.labels);
	assert!(!has(&drawn, "Go to Contacts"));
	phone.click_label("Find nearby users");
	assert_eq!(phone.app.page(), Page::Connect);
}

#[tokio::test]
async fn the_buttons_below_a_full_grid_navigate() {
	let mut phone = rig(360.0, &state(6, 6));
	let drawn = phone.page(Page::Calls);
	let (grid_bottom, buttons_top) = (
		rect_of(&drawn, "Flo").max.y,
		rect_of(&drawn, "Go to Contacts").min.y,
	);
	assert!(buttons_top > grid_bottom, "the buttons sit below the grid");
	phone.click_label("Find nearby users");
	assert_eq!(phone.app.page(), Page::Connect);
}

#[tokio::test]
async fn a_sidebar_online_list_is_not_shown_next_to_the_grid() {
	let mut phone = rig(720.0, &state(2, 4));
	let drawn = phone.page(Page::Calls);
	assert!(!has(&drawn, "Online now"));
	// Only the favorites are on the Calls page; the other contacts are not.
	assert!(!has(&drawn, "Cid") && !has(&drawn, "Dee"));
}
