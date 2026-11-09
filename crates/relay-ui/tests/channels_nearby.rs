//! "Channels on your network" on the Channels page: the rows, Join by name, the form a refused
//! join opens, the empty state, touch size and fit in every language.

#![allow(clippy::await_holding_lock)]

mod common;
mod phone;

use std::sync::Arc;
use std::time::Duration;

use common::FakeOps;
use phone::{Drawn, Phone};
use relay_ui::engine::Command;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::{AppState, Link, NearbyChannel};

static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turn() -> std::sync::MutexGuard<'static, ()> {
	TURN.lock().unwrap_or_else(|e| e.into_inner())
}

fn nearby(name: &str, devices: u32, ambiguous: bool) -> NearbyChannel {
	NearbyChannel {
		name: name.into(),
		devices,
		ambiguous,
		password: false,
	}
}

fn rig(width: f32, language: Language, list: Vec<NearbyChannel>) -> (Phone, Arc<FakeOps>) {
	let mut phone = Phone::new(width, 5000.0, language);
	let mut state: AppState = phone::busy_state();
	state.channels.clear();
	state.nearby_channels = list;
	phone.show_state(&state);
	phone.actions.hub.update(|s| *s = state.clone());
	let ops = Arc::new(FakeOps::default());
	phone.slot.set(ops.clone());
	phone.page(Page::Channels);
	(phone, ops)
}

fn count(drawn: &Drawn, text: &str) -> usize {
	drawn.labels.iter().filter(|l| *l == text).count()
}

fn top(drawn: &Drawn, text: &str) -> f32 {
	drawn
		.named
		.iter()
		.find(|(t, _)| t == text)
		.unwrap_or_else(|| panic!("{text:?} not drawn: {:?}", drawn.labels))
		.1
		.top()
}

fn three() -> Vec<NearbyChannel> {
	vec![
		nearby("Club", 1, false),
		nearby("Garage", 2, false),
		nearby("Lobby", 2, true),
	]
}

/// Click the "Join" button on the row of `name`.
fn click_join_row(phone: &mut Phone, name: &str) {
	let drawn = phone.settle();
	let y = top(&drawn, name);
	let (_, rect) = drawn
		.named
		.iter()
		.filter(|(t, _)| t == "Join")
		.min_by(|a, b| (a.1.top() - y).abs().total_cmp(&(b.1.top() - y).abs()))
		.expect("a Join button");
	phone.click_at(rect.center());
}

#[tokio::test]
async fn rows_show_name_and_plural_device_count_below_the_actions() {
	let _turn = turn();
	let (mut phone, _ops) = rig(720.0, Language::En, three());
	let drawn = phone.settle();
	assert_eq!(count(&drawn, "Channels on your network"), 1);
	assert!(top(&drawn, "Join a channel") < top(&drawn, "Channels on your network"));
	assert!(top(&drawn, "Channels on your network") < top(&drawn, "Club"));
	assert_eq!(count(&drawn, "1 device"), 1);
	assert_eq!(count(&drawn, "2 devices"), 1);
	assert_eq!(
		count(&drawn, "Join"),
		2,
		"an ambiguous name has no Join button"
	);
	assert_eq!(
		count(
			&drawn,
			"Two channels use this name. Join with a code instead."
		),
		1
	);
	assert_eq!(count(&drawn, "No channels found on your network yet"), 0);
}

#[tokio::test]
async fn join_sends_the_name_with_no_password() {
	let _turn = turn();
	let (mut phone, ops) = rig(720.0, Language::En, three());
	click_join_row(&mut phone, "Garage");
	tokio::time::sleep(Duration::from_millis(100)).await;
	let sent = ops.recorded();
	assert_eq!(sent.len(), 1, "{sent:?}");
	let Command::JoinChannel {
		channel_id,
		name,
		password,
		invite,
	} = &sent[0]
	else {
		panic!("wrong command: {sent:?}");
	};
	assert!(channel_id.is_empty() && invite.expose().is_empty());
	assert_eq!(name, "Garage");
	assert_eq!(password.expose(), "");
	let drawn = phone.settle();
	assert!(
		!drawn
			.labels
			.iter()
			.any(|l| l.starts_with("Paste an invite")),
		"a join that worked opens no form"
	);
}

#[tokio::test]
async fn a_refused_join_opens_the_join_form_with_the_name_in_place() {
	let _turn = turn();
	let (mut phone, ops) = rig(720.0, Language::En, three());
	*ops.fail_with.lock().unwrap() = Some("denied".into());
	click_join_row(&mut phone, "Club");
	tokio::time::sleep(Duration::from_millis(100)).await;
	let drawn = phone.settle();
	assert!(
		drawn
			.labels
			.iter()
			.any(|l| l.starts_with("Paste an invite"))
	);
	assert_eq!(
		count(&drawn, "Club"),
		2,
		"the name is in the row and in the join field"
	);

	// Entering a password and pressing Join sends the same name with it.
	*ops.fail_with.lock().unwrap() = None;
	ops.commands.lock().unwrap().clear();
	let controller = phone.actions.controller.clone();
	controller.join_by_name("Club", "hunter22").await.unwrap();
	let sent = ops.recorded();
	let Command::JoinChannel { name, password, .. } = &sent[0] else {
		panic!("wrong command");
	};
	assert_eq!((name.as_str(), password.expose()), ("Club", "hunter22"));
}

#[tokio::test]
async fn nothing_found_is_one_calm_line_and_offline_says_why() {
	let _turn = turn();
	let (mut phone, _ops) = rig(720.0, Language::En, Vec::new());
	let drawn = phone.settle();
	assert_eq!(count(&drawn, "No channels found on your network yet"), 1);
	assert!(
		!drawn.labels.iter().any(|l| l.contains("offline")),
		"online, no reason is given"
	);
	assert_eq!(count(&drawn, "Join"), 0);

	let mut state = phone::busy_state();
	state.channels.clear();
	state.link = Link::Offline;
	phone.show_state(&state);
	let drawn = phone.settle();
	assert_eq!(count(&drawn, "No channels found on your network yet"), 1);
	assert_eq!(
		count(
			&drawn,
			"This device is offline, so it cannot see other devices."
		),
		1
	);
}

#[tokio::test]
async fn rows_keep_touch_size_and_stay_on_the_phone_screen() {
	let _turn = turn();
	let (mut phone, _ops) = rig(360.0, Language::En, three());
	let drawn = phone.settle();
	let screen = phone.screen().expand(1.0);
	let joins = drawn.named.iter().filter(|(t, _)| t == "Join").count();
	assert_eq!(joins, 2);
	for r in drawn.tappable.iter().filter(|r| r.width() > 12.0) {
		assert!(
			r.height() >= 43.5 && r.width() >= 43.5,
			"small target {:?}",
			r.size()
		);
	}
	for r in &drawn.widgets {
		assert!(
			r.min.x >= screen.min.x && r.max.x <= screen.max.x,
			"overflow {r:?}"
		);
	}
}

#[tokio::test]
async fn every_language_fits_rows_and_the_empty_state() {
	let _turn = turn();
	let mut bad = Vec::new();
	let long = "A very long channel name 32 chr.";
	assert_eq!(long.chars().count(), 32);
	for language in Language::ALL {
		for width in [720.0, 360.0, 320.0] {
			for (list, link) in [
				(
					vec![nearby(long, 12, false), nearby("Lobby", 1, true)],
					Link::Connected,
				),
				(Vec::new(), Link::Connected),
				(Vec::new(), Link::Offline),
			] {
				let (mut phone, _ops) = rig(width, language, list.clone());
				let mut state = phone::busy_state();
				state.channels.clear();
				state.nearby_channels = list;
				state.link = link;
				phone.show_state(&state);
				let drawn = phone.settle();
				let screen = phone.screen().expand(1.0);
				for r in drawn.widgets.iter().chain(&drawn.painted) {
					if r.min.x < screen.min.x || r.max.x > screen.max.x {
						bad.push(format!("{language:?} {width} {link:?}: {r:?}"));
					}
				}
			}
		}
	}
	assert!(bad.is_empty(), "{bad:#?}");
}
