//! Answer automatically on the contact card: where the menu entry sits, the confirmation, the
//! greyed entry for a contact that is not verified, the line in the card, and the words of the
//! engine's refusal.

#![allow(clippy::await_holding_lock)]

mod common;
mod phone;

use std::sync::Arc;

use common::{FakeOps, controller_with};
use phone::{Drawn, Phone};
use relay_ui::engine::Command;
use relay_ui::i18n::Language;
use relay_ui::icons::Icon;
use relay_ui::layout::Page;
use relay_ui::model::{AppState, Peer, Phase, Trust};

static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turn() -> std::sync::MutexGuard<'static, ()> {
	TURN.lock().unwrap_or_else(|e| e.into_inner())
}

fn contact(trust: Trust, auto_answer: bool) -> Peer {
	let mut peer = phone::busy_state().peers[0].clone();
	peer.peer_id = hex::encode([11; 32]);
	peer.display_name = "Ada".into();
	peer.trust = trust;
	peer.reachable = true;
	peer.favorite = false;
	peer.auto_answer = auto_answer;
	peer
}

fn rig(width: f32, height: f32, language: Language, peer: Peer) -> (Phone, Arc<FakeOps>) {
	let mut phone = Phone::new(width, height, language);
	let mut state: AppState = phone::busy_state();
	state.calls.clear();
	state.peers = vec![peer];
	phone.show_state(&state);
	phone.actions.hub.update(|s| *s = state.clone());
	let ops = Arc::new(FakeOps::default());
	phone.slot.set(ops.clone());
	phone.page(Page::Contacts);
	(phone, ops)
}

fn y_of(drawn: &Drawn, text: &str) -> f32 {
	drawn
		.named
		.iter()
		.find(|(t, _)| t == text)
		.unwrap_or_else(|| panic!("no {text:?} in {:?}", drawn.labels))
		.1
		.center()
		.y
}

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

async fn recorded(ops: &FakeOps) -> Vec<Command> {
	for _ in 0..100 {
		if !ops.recorded().is_empty() {
			break;
		}
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;
	}
	ops.recorded()
}

#[tokio::test]
async fn the_entry_sits_above_forget_and_block() {
	let _turn = turn();
	let (mut phone, _ops) = rig(360.0, 1400.0, Language::En, contact(Trust::Verified, false));
	phone.click_icon_nth(Icon::More, 0);
	let menu = phone.settle();
	let auto = y_of(&menu, "Answer automatically");
	assert!(
		auto < y_of(&menu, "Forget this device"),
		"{:?}",
		menu.labels
	);
	assert!(auto < y_of(&menu, "Block device"));
}

#[tokio::test]
async fn switching_on_asks_first_and_yes_sends_it() {
	let _turn = turn();
	let (mut phone, ops) = rig(360.0, 1400.0, Language::En, contact(Trust::Verified, false));
	phone.click_icon_nth(Icon::More, 0);
	phone.click_label("Answer automatically");
	let asked = phone.settle();
	assert!(
		asked
			.labels
			.iter()
			.any(|l| l.starts_with("Ada will be able to call you")
				&& l.contains("Default when a call connects")
				&& l.ends_with("people you trust.")),
		"{:?}",
		asked.labels
	);
	assert!(ops.recorded().is_empty(), "nothing is sent before Yes");
	phone.click_label("Yes, answer automatically");
	assert_eq!(
		recorded(&ops).await,
		[Command::SetPeerAutoAnswer {
			peer_id: vec![11; 32],
			enabled: true
		}]
	);
	assert!(!has(&phone.settle(), "Yes, answer automatically"));
}

#[tokio::test]
async fn cancel_sends_nothing_and_closes_the_question() {
	let _turn = turn();
	let (mut phone, ops) = rig(360.0, 1400.0, Language::En, contact(Trust::Verified, false));
	phone.click_icon_nth(Icon::More, 0);
	phone.click_label("Answer automatically");
	phone.click_label("Cancel");
	let drawn = phone.settle();
	assert!(
		!has(&drawn, "Yes, answer automatically"),
		"{:?}",
		drawn.labels
	);
	assert!(ops.recorded().is_empty());
}

#[tokio::test]
async fn a_contact_that_is_not_verified_is_greyed_with_the_reason() {
	let _turn = turn();
	let (mut phone, ops) = rig(
		360.0,
		1400.0,
		Language::En,
		contact(Trust::TofuUnverified, false),
	);
	phone.click_icon_nth(Icon::More, 0);
	let menu = phone.settle();
	assert!(has(&menu, "Verify this device first"), "{:?}", menu.labels);
	let entry = menu
		.named
		.iter()
		.find(|(t, _)| t == "Answer automatically")
		.expect("the entry is listed")
		.1;
	assert!(
		!menu
			.tappable
			.iter()
			.any(|t| t.contains(entry.center()) && t.height() < 100.0),
		"greyed"
	);
	assert!(ops.recorded().is_empty());
}

#[tokio::test]
async fn while_on_the_card_says_so_and_the_entry_switches_it_off() {
	let _turn = turn();
	let (mut phone, ops) = rig(360.0, 1400.0, Language::En, contact(Trust::Verified, true));
	let card = phone.settle();
	assert!(has(&card, "Answers automatically"), "{:?}", card.labels);
	phone.click_icon_nth(Icon::More, 0);
	let menu = phone.settle();
	assert!(has(&menu, "Answer automatically"), "{:?}", menu.labels);
	phone.click_label("Answer automatically");
	assert_eq!(
		recorded(&ops).await,
		[Command::SetPeerAutoAnswer {
			peer_id: vec![11; 32],
			enabled: false
		}]
	);
}

#[tokio::test]
async fn a_call_answered_automatically_says_so() {
	let _turn = turn();
	let mut phone = Phone::new(360.0, 1400.0, Language::En);
	let mut state = phone::busy_state();
	state.calls.retain(|c| c.phase == Phase::Connected);
	state.calls[0].auto_answered = true;
	phone.show_state(&state);
	let drawn = phone.page(Page::Calls);
	assert!(has(&drawn, "Answered automatically"), "{:?}", drawn.labels);
	state.calls[0].auto_answered = false;
	phone.show_state(&state);
	assert!(!has(&phone.settle(), "Answered automatically"));
}

#[tokio::test]
async fn the_engines_refusal_is_put_in_plain_words() {
	let _turn = turn();
	let (controller, hub, ops) = controller_with(common::snapshot(1, vec![]), 1);
	hub.update(|s| s.peers[0].trust = Trust::Verified);
	*ops.fail_with.lock().unwrap() =
		Some("Only verified contacts can answer automatically".to_owned());
	*ops.fail_code.lock().unwrap() = Some("INVALID_REQUEST".to_owned());
	let id = hex::encode(common::PEER);
	let error = controller
		.set_peer_auto_answer(&id, true)
		.await
		.unwrap_err();
	assert_eq!(
		error.to_string(),
		relay_ui::i18n::t("err-auto-answer-verified")
	);
	// Not verified: refused here, nothing is sent.
	hub.update(|s| s.peers[0].trust = Trust::TofuUnverified);
	ops.commands.lock().unwrap().clear();
	assert!(controller.set_peer_auto_answer(&id, true).await.is_err());
	assert!(ops.recorded().is_empty());
}

#[tokio::test]
async fn the_menu_and_question_fit_in_every_language_at_every_size() {
	let _turn = turn();
	let mut bad = Vec::new();
	for language in phone::LANGUAGES {
		for (desktop, w, h) in [
			(false, 360.0, 800.0),
			(false, 320.0, 640.0),
			(true, 720.0, 800.0),
		] {
			for trust in [Trust::Verified, Trust::TofuUnverified] {
				let (mut phone, _ops) = {
					let (p, o) = rig(w, h, language, contact(trust, false));
					if desktop {
						p.shell
							.desktop
							.store(true, std::sync::atomic::Ordering::SeqCst);
					}
					(p, o)
				};
				phone.click_icon_nth(Icon::More, 0);
				let mut frames = vec![phone.settle()];
				if trust == Trust::Verified {
					phone.click_label(relay_ui::i18n::t("contact-auto-answer"));
					frames.push(phone.settle());
				}
				for drawn in frames {
					let screen = phone.screen().expand(1.0);
					for r in drawn.widgets.iter().chain(&drawn.painted) {
						if r.min.x < screen.min.x || r.max.x > screen.max.x {
							bad.push(format!("{language:?} {w}x{h}: overflow {r:?}"));
						}
					}
					if !desktop {
						for r in drawn.tappable.iter().filter(|r| r.width() > 12.0) {
							if r.height() < 43.5 || r.width() < 43.5 {
								bad.push(format!("{language:?} {w}x{h}: small {:?}", r.size()));
							}
						}
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
