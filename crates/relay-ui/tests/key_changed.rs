//! A contact whose security key changed: the card, its three choices, the confirm step that
//! enables Trust only for the exact fingerprint, and the commands that follow.

mod common;
mod phone;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::FakeOps;
use phone::{Drawn, Phone};
use relay_ui::engine::Command;
use relay_ui::i18n::{self, Language};
use relay_ui::icons::Icon;
use relay_ui::layout::Page;
use relay_ui::model::{AppState, Peer, Trust};

const NEW: &str = "59f1";
const OLD: &str = "1a10";

fn digest(prefix: &str) -> String {
	prefix.repeat(16)
}

fn changed_peer() -> Peer {
	Peer {
		peer_id: digest(NEW),
		display_name: "Dantes-Laptop.local".into(),
		addresses: Vec::new(),
		trust: Trust::KeyChanged,
		reachable: false,
		avatar_hash: String::new(),
		fingerprint_short: "59F1-59F1-59F1-59F1-59F1".into(),
		port: 0,
		last_seen_unix_ms: 0,
		shared_channels: Vec::new(),
		favorite: true,
		favorite_position: 0,
		key_changed: true,
		previous_peer_id: digest(OLD),
		previous_fingerprint_short: "1A10-1A10-1A10-1A10-1A10".into(),
	}
}

fn state(with_card: bool) -> AppState {
	let mut state = phone::busy_state();
	state.peers.retain(|p| !p.key_changed);
	if with_card {
		state.peers.push(changed_peer());
	}
	state
}

fn rig(
	width: f32,
	height: f32,
	language: Language,
	desktop: bool,
	scanner: bool,
) -> (Phone, Arc<FakeOps>) {
	let mut phone = if desktop {
		Phone::desktop(width, height, language)
	} else {
		Phone::new(width, height, language)
	};
	let ops = Arc::new(FakeOps::default());
	phone.slot.set(ops.clone());
	phone.shell.scanner.store(scanner, Ordering::SeqCst);
	let state = state(true);
	phone.show_state(&state);
	phone.actions.hub.update(|s| *s = state.clone());
	phone.page(Page::Contacts);
	(phone, ops)
}

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l.contains(text))
}

fn t(id: &'static str) -> &'static str {
	i18n::t(id)
}

async fn type_text(phone: &mut Phone, text: &str) {
	let drawn = phone.settle();
	let (_, hint) = drawn
		.named
		.iter()
		.find(|(label, _)| label == t("keychange-field-hint"))
		.expect("the field")
		.clone();
	phone.click_at(hint.center());
	phone.frame_with(vec![egui::Event::Text(text.to_owned())]);
	phone.settle();
}

#[tokio::test]
async fn the_card_shows_only_for_a_contact_with_a_changed_key() {
	let (mut phone, _) = rig(360.0, 1800.0, Language::En, false, false);
	let drawn = phone.settle();
	assert!(
		has(&drawn, "Dantes-Laptop's security key changed"),
		"{:?}",
		drawn.labels
	);
	assert!(has(
		&drawn,
		"This usually happens after Relay was reinstalled or reset on that device. Calls are blocked until you verify it again."
	));
	assert!(!has(&drawn, "Key changed: do not trust"));
	assert!(!has(&drawn, "computer"));
	// No key digits on the card itself.
	assert!(!has(&drawn, "59F1"));
	// The other contacts are plain cards with a Call button.
	assert!(has(&drawn, "Ada Lovelace"));
	assert!(!has(&drawn, "Ada Lovelace's security key changed"));

	let mut calm = state(false);
	calm.peers.iter_mut().for_each(|p| p.key_changed = false);
	phone.show_state(&calm);
	let drawn = phone.settle();
	assert!(!has(&drawn, "security key changed"), "{:?}", drawn.labels);
	assert!(has(&drawn, "Call"));
}

#[tokio::test]
async fn the_card_sorts_first_hides_call_and_keeps_the_star() {
	let (mut phone, _) = rig(360.0, 3000.0, Language::En, false, false);
	let drawn = phone.settle();
	let at = |text: &str| {
		drawn
			.named
			.iter()
			.find(|(label, _)| label.contains(text))
			.map(|(_, r)| r.min.y)
			.unwrap_or_else(|| panic!("no {text:?} in {:?}", drawn.labels))
	};
	assert!(at("security key changed") < at("Grace Hopper"));
	// Alone, the card has no Call button and still has the star.
	let mut alone = state(false);
	alone.peers.push(changed_peer());
	alone.peers.retain(|p| p.key_changed);
	phone.show_state(&alone);
	let drawn = phone.settle();
	assert!(
		!drawn.labels.iter().any(|l| l == "Call"),
		"{:?}",
		drawn.labels
	);
	assert!(phone::has_icon(&drawn, Icon::StarFilled));
}

#[tokio::test]
async fn scan_is_offered_only_where_the_shell_can_scan() {
	let (mut phone, _) = rig(360.0, 1800.0, Language::En, false, false);
	let drawn = phone.settle();
	assert!(!has(&drawn, "Scan their QR code"));
	assert!(has(&drawn, "Compare and trust new key"));
	assert!(has(&drawn, "Remove contact"));

	let (mut phone, _) = rig(360.0, 1800.0, Language::En, false, true);
	let drawn = phone.settle();
	assert!(has(&drawn, "Scan their QR code"));
	phone.click_label("Scan their QR code");
	tokio::time::sleep(Duration::from_millis(50)).await;
	assert_eq!(phone.shell.scans.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn trust_is_enabled_only_for_the_exact_fingerprint() {
	let (mut phone, ops) = rig(360.0, 2400.0, Language::En, false, false);
	phone.click_label("Compare and trust new key");
	let drawn = phone.settle();
	assert!(has(&drawn, "New key seen from this device"));
	assert!(has(&drawn, "59F1 59F1 59F1 59F1"), "{:?}", drawn.labels);

	// Part of it, another device's, or one wrong digit: nothing is sent.
	let full = digest(NEW);
	let wrong_last = format!("{}0", &full[..63]);
	for typed in [&full[..32], &digest(OLD), &wrong_last] {
		type_text(&mut phone, typed).await;
		phone.click_label("Trust new key");
		assert!(ops.recorded().is_empty(), "{typed} must not enable Trust");
		phone.click_label("Not now");
		phone.click_label("Compare and trust new key");
	}

	// Spaces, dashes and capitals are what people type.
	let grouped: Vec<String> = full
		.as_bytes()
		.chunks(4)
		.map(|c| String::from_utf8_lossy(c).to_uppercase())
		.collect();
	type_text(&mut phone, &grouped.join("-")).await;
	assert!(has(&phone.settle(), "They match."));
	phone.click_label("Trust new key");
	tokio::time::sleep(Duration::from_millis(50)).await;
	assert_eq!(
		ops.recorded(),
		[Command::ReplacePinnedKey {
			peer_id: [0x59, 0xf1].repeat(16),
			fingerprint: full,
		}]
	);
}

#[tokio::test]
async fn removing_the_contact_forgets_the_stored_pinned_key() {
	let (mut phone, ops) = rig(360.0, 2400.0, Language::En, false, false);
	phone.click_label("Remove contact");
	let drawn = phone.settle();
	assert!(
		has(&drawn, "Remove Dantes-Laptop from your contacts?"),
		"{:?}",
		drawn.labels
	);
	assert!(ops.recorded().is_empty(), "asking changes nothing");
	phone.click_label("Yes, remove");
	tokio::time::sleep(Duration::from_millis(50)).await;
	assert_eq!(
		ops.recorded(),
		[Command::ForgetPeer {
			peer_id: [0x1a, 0x10].repeat(16)
		}]
	);
}

#[tokio::test]
async fn an_idle_card_asks_for_no_repaint() {
	let (mut phone, _) = rig(360.0, 800.0, Language::En, false, true);
	let mut quiet = state(true);
	quiet.calls.clear();
	phone.show_state(&quiet);
	for _ in 0..3 {
		phone.settle();
	}
	let input = egui::RawInput {
		screen_rect: Some(phone.screen()),
		..Default::default()
	};
	let app = &mut phone.app;
	let out = phone.ctx.run_ui(input, |ui| {
		app.begin_frame(ui.ctx());
		app.ui(ui);
	});
	let delay = out
		.viewport_output
		.get(&egui::ViewportId::ROOT)
		.map_or(Duration::MAX, |v| v.repaint_delay);
	assert_eq!(delay, Duration::MAX);
}

#[tokio::test]
async fn every_step_fits_with_touch_sized_targets_in_every_language() {
	let sizes = [
		(false, 360.0, 800.0),
		(false, 320.0, 640.0),
		(true, 720.0, 800.0),
	];
	let mut bad = Vec::new();
	for language in [Language::En, Language::De, Language::Ru] {
		for (desktop, w, h) in sizes {
			for step in ["idle", "compare", "typed", "remove"] {
				let (mut phone, _) = rig(w, h * 3.0, language, desktop, true);
				match step {
					"compare" | "typed" => {
						phone.click_label(t("keychange-compare"));
						if step == "typed" {
							type_text(&mut phone, &digest(NEW)).await;
						}
					}
					"remove" => phone.click_label(t("keychange-remove")),
					_ => {}
				}
				let drawn = phone.settle();
				let tag = format!("{language:?} {w}x{h} {step}");
				let screen = phone.screen().expand(1.0);
				let texts = drawn.texts.iter().map(|(r, _)| r);
				for r in drawn.widgets.iter().chain(texts) {
					if r.min.x < screen.min.x || r.max.x > screen.max.x {
						bad.push(format!("{tag}: overflow {r:?}"));
					}
				}
				if !desktop {
					for r in drawn.tappable.iter().filter(|r| r.width() > 12.0) {
						if r.height() < 43.5 || r.width() < 43.5 {
							bad.push(format!("{tag}: small target {:?}", r.size()));
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
