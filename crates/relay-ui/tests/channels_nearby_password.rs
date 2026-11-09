//! Password channels in "Channels on your network" (lock, inline prompt) and the Create form's
//! Listed / Invite only exclusivity.

#![allow(clippy::await_holding_lock)]

mod common;
mod phone;

use std::sync::Arc;
use std::time::Duration;

use common::FakeOps;
use phone::{Drawn, Phone, has_icon};
use relay_ui::engine::Command;
use relay_ui::i18n::Language;
use relay_ui::icons::Icon;
use relay_ui::layout::Page;
use relay_ui::model::{AppState, NearbyChannel};

static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turn() -> std::sync::MutexGuard<'static, ()> {
	TURN.lock().unwrap_or_else(|e| e.into_inner())
}

fn list() -> Vec<NearbyChannel> {
	vec![
		NearbyChannel {
			name: "Garage".into(),
			devices: 2,
			ambiguous: false,
			password: false,
		},
		NearbyChannel {
			name: "Vault".into(),
			devices: 1,
			ambiguous: false,
			password: true,
		},
	]
}

fn rig(width: f32, language: Language) -> (Phone, Arc<FakeOps>) {
	let mut phone = Phone::new(width, 5000.0, language);
	let mut state: AppState = phone::busy_state();
	state.channels.clear();
	state.nearby_channels = list();
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

fn join_on_row(phone: &mut Phone, name: &str) {
	let drawn = phone.settle();
	let y = drawn.named.iter().find(|(t, _)| t == name).unwrap().1.top();
	let (_, rect) = drawn
		.named
		.iter()
		.filter(|(t, _)| t == relay_ui::i18n::t("action-join"))
		.min_by(|a, b| (a.1.top() - y).abs().total_cmp(&(b.1.top() - y).abs()))
		.unwrap();
	phone.click_at(rect.center());
}

fn key(key: egui::Key) -> Vec<egui::Event> {
	vec![egui::Event::Key {
		key,
		physical_key: Some(key),
		pressed: true,
		repeat: false,
		modifiers: Default::default(),
	}]
}

async fn pause() {
	tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn a_password_channel_shows_a_lock_and_asks_before_joining() {
	let _turn = turn();
	let (mut phone, ops) = rig(720.0, Language::En);
	let drawn = phone.settle();
	assert!(has_icon(&drawn, Icon::Lock), "the lock marks a password");
	assert_eq!(count(&drawn, "Cancel"), 0);

	join_on_row(&mut phone, "Vault");
	pause().await;
	let drawn = phone.settle();
	assert!(
		ops.recorded().is_empty(),
		"nothing is sent before the password"
	);
	assert_eq!(count(&drawn, "Cancel"), 1);
	assert_eq!(count(&drawn, "Password"), 1, "the masked field is open");

	// Type the password and press Enter.
	phone.frame_with(vec![egui::Event::Text("hunter22!".into())]);
	phone.frame_with(key(egui::Key::Enter));
	pause().await;
	let sent = ops.recorded();
	let Command::JoinChannel { name, password, .. } = &sent[0] else {
		panic!("wrong command: {sent:?}");
	};
	assert_eq!((name.as_str(), password.expose()), ("Vault", "hunter22!"));
}

#[tokio::test]
async fn a_wrong_password_keeps_the_prompt_open_and_escape_cancels() {
	let _turn = turn();
	let (mut phone, ops) = rig(720.0, Language::En);
	*ops.fail_with.lock().unwrap() = Some("denied".into());
	join_on_row(&mut phone, "Vault");
	phone.settle();
	phone.frame_with(vec![egui::Event::Text("wrongpass".into())]);
	phone.settle();
	// The prompt's own Join is the second one on the page after the row's.
	let drawn = phone.settle();
	let mut joins: Vec<_> = drawn.named.iter().filter(|(t, _)| t == "Join").collect();
	joins.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
	phone.click_at(joins.last().unwrap().1.center());
	pause().await;
	let drawn = phone.settle();
	assert_eq!(ops.recorded().len(), 1);
	assert_eq!(count(&drawn, "Cancel"), 1, "the prompt stays open");
	assert!(
		!drawn
			.labels
			.iter()
			.any(|l| l.starts_with("Paste an invite")),
		"a password refusal does not open the Join form"
	);

	phone.frame_with(key(egui::Key::Escape));
	let drawn = phone.settle();
	assert_eq!(count(&drawn, "Cancel"), 0);
}

#[tokio::test]
async fn an_open_channel_still_joins_at_once() {
	let _turn = turn();
	let (mut phone, ops) = rig(720.0, Language::En);
	join_on_row(&mut phone, "Garage");
	pause().await;
	assert_eq!(ops.recorded().len(), 1);
	assert_eq!(count(&phone.settle(), "Cancel"), 0);
}

#[tokio::test]
async fn the_prompt_keeps_touch_size_on_the_phone() {
	let _turn = turn();
	let (mut phone, _ops) = rig(360.0, Language::En);
	join_on_row(&mut phone, "Vault");
	phone.settle();
	let drawn = phone.settle();
	let screen = phone.screen().expand(1.0);
	assert_eq!(count(&drawn, "Cancel"), 1);
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
async fn invite_only_switches_listed_off_and_says_why() {
	let _turn = turn();
	let (mut phone, _ops) = rig(720.0, Language::En);
	phone.click_label("Create a channel");
	phone.click_label("Listed: show the name to members' devices");
	phone.click_label("Invite only");
	let drawn = phone.settle();
	let reason = "Invite-only channels are never listed on the network.";
	assert_eq!(count(&drawn, reason), 1);
	let (_, listed) = drawn
		.named
		.iter()
		.find(|(t, _)| t == "Listed: show the name to members' devices")
		.unwrap();
	assert!(
		!drawn.tappable.iter().any(|r| r.contains(listed.center())),
		"Listed cannot be changed while Invite only is on"
	);
	// Turning invite-only off frees it again, and the reason goes.
	phone.click_label("Invite only");
	let drawn = phone.settle();
	assert_eq!(count(&drawn, reason), 0);
	let (_, listed) = drawn
		.named
		.iter()
		.find(|(t, _)| t == "Listed: show the name to members' devices")
		.unwrap();
	assert!(drawn.tappable.iter().any(|r| r.contains(listed.center())));
}

#[tokio::test]
async fn every_language_fits_the_prompt_and_the_exclusive_options() {
	let _turn = turn();
	let mut bad = Vec::new();
	for language in Language::ALL {
		for width in [720.0, 360.0, 320.0] {
			let (mut phone, _ops) = rig(width, language);
			join_on_row(&mut phone, "Vault");
			phone.settle();
			let mut drawn = phone.settle();
			// The create form with Invite only on, as the same page.
			phone.click_label(relay_ui::i18n::t("channel-create-heading"));
			phone.click_label(relay_ui::i18n::t("channel-invite-only-label"));
			drawn.widgets.extend(phone.settle().widgets);
			let screen = phone.screen().expand(1.0);
			for r in drawn.widgets.iter() {
				if r.min.x < screen.min.x || r.max.x > screen.max.x {
					bad.push(format!("{language:?} {width}: {r:?}"));
				}
			}
		}
	}
	assert!(bad.is_empty(), "{bad:#?}");
}
