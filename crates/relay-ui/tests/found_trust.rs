//! Devices found on the network but not trusted: the Nearby section of Connect, a
//! confirm step that points at the fingerprint check, and the trust decision
//! that goes to the engine with the full key digest.

mod common;
mod phone;

use std::sync::Arc;

use common::FakeOps;
use phone::{LANGUAGES, Phone};
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::{Peer, Problem, Trust};

fn stranger() -> Peer {
	Peer {
		peer_id: "ab".repeat(32),
		display_name: "Dantes Laptop".into(),
		addresses: vec!["192.168.1.77:7000".into()],
		trust: Trust::Unknown,
		reachable: false,
		avatar_hash: String::new(),
		fingerprint_short: "ABAB-ABAB-ABAB-ABAB-ABAB".into(),
		shared_channels: Vec::new(),
		favorite: false,
		favorite_position: 0,
		port: 7000,
		last_seen_unix_ms: 0,
	}
}

fn rig(width: f32, height: f32, language: Language) -> (Phone, Arc<FakeOps>) {
	let mut phone = Phone::desktop(width, height, language);
	let ops = Arc::new(FakeOps::default());
	phone.slot.set(ops.clone());
	let mut state = phone::busy_state();
	state.peers.push(stranger());
	phone.show_state(&state);
	// The controller checks the device against the hub's state.
	phone.actions.hub.update(|s| s.peers.push(stranger()));
	phone.page(Page::Connect);
	(phone, ops)
}

fn has(labels: &[String], text: &str) -> bool {
	labels.iter().any(|l| l.contains(text))
}

fn position(labels: &[String], text: &str) -> usize {
	labels
		.iter()
		.position(|l| l.contains(text))
		.unwrap_or_else(|| panic!("no {text:?} in {labels:?}"))
}

#[tokio::test]
async fn an_untrusted_device_is_nearby_with_its_address_and_not_among_contacts() {
	let (mut phone, _ops) = rig(720.0, 1800.0, Language::En);
	let drawn = phone.settle();
	let labels = &drawn.labels;
	assert!(position(labels, "Nearby") < position(labels, "Dantes Laptop"));
	assert!(has(labels, "Not trusted yet"));
	assert!(has(labels, "Trust this device"));
	assert!(
		has(labels, "192.168.1.77") && !has(labels, "192.168.1.77:7000"),
		"the local address without the port: {labels:?}"
	);
	// A trusted device lives in Contacts only.
	assert!(!has(labels, "Online · Verified"), "{labels:?}");
	let contacts = phone.page(Page::Contacts);
	assert!(has(&contacts.labels, "Online · Verified"));
	assert!(
		!has(&contacts.labels, "Dantes Laptop"),
		"{:?}",
		contacts.labels
	);
	assert!(!has(&contacts.labels, "Trust this device"));
}

#[tokio::test]
async fn trusting_asks_first_then_sends_the_digest() {
	let (mut phone, ops) = rig(720.0, 1800.0, Language::En);
	phone.click_label("Trust this device");
	let drawn = phone.settle();
	assert!(has(&drawn.labels, "compare this fingerprint"));
	assert!(has(&drawn.labels, &"ABAB ".repeat(15)));
	assert!(
		ops.recorded().is_empty(),
		"nothing is sent before the confirm"
	);
	phone.click_label("Yes, trust it");
	tokio::task::yield_now().await;
	tokio::time::sleep(std::time::Duration::from_millis(100)).await;
	assert_eq!(
		ops.recorded(),
		vec![relay_ui::engine::Command::TrustPeer {
			peer_id: vec![0xab; 32]
		}]
	);
}

#[tokio::test]
async fn not_now_sends_nothing_and_goes_back() {
	let (mut phone, ops) = rig(720.0, 1800.0, Language::En);
	phone.click_label("Trust this device");
	phone.click_label("Not now");
	let drawn = phone.settle();
	assert!(has(&drawn.labels, "Trust this device"));
	tokio::time::sleep(std::time::Duration::from_millis(50)).await;
	assert!(ops.recorded().is_empty());
}

#[tokio::test]
async fn the_engines_trust_warning_is_not_shown_anywhere() {
	for page in [Page::Calls, Page::Contacts, Page::Connect, Page::Channels] {
		let mut phone = Phone::desktop(720.0, 1800.0, Language::En);
		let mut state = phone::busy_state();
		state.peers.push(stranger());
		state.warnings.push(Problem {
			code: "TRUST_REQUIRED".into(),
			message: "`Dantes Laptop` (ABAB) is waiting for a trust decision".into(),
			retryable: false,
			recovery_action: "decide_peer_trust".into(),
		});
		phone.show_state(&state);
		let drawn = phone.page(page);
		assert!(
			!has(&drawn.labels, "waiting for a trust decision"),
			"{page:?}"
		);
		assert!(!has(&drawn.labels, "Review"), "{page:?}");
	}
}

#[tokio::test]
async fn other_warnings_still_show() {
	let mut phone = Phone::desktop(720.0, 1800.0, Language::En);
	let mut state = phone::busy_state();
	state.warnings.push(Problem {
		code: "PEER_KEY_CHANGED".into(),
		message: "A device changed its key".into(),
		retryable: false,
		recovery_action: String::new(),
	});
	phone.show_state(&state);
	let drawn = phone.page(Page::Calls);
	assert!(has(&drawn.labels, "A device's security key changed"));
}

#[tokio::test]
async fn the_section_fits_in_every_language_and_width() {
	let mut bad = Vec::new();
	for language in LANGUAGES {
		for (desktop, w, h) in [(true, 720.0, 800.0), (false, 360.0, 800.0)] {
			for confirm in [false, true] {
				// Clicking needs the button on screen, so the confirm step gets a tall window;
				// only horizontal overflow is checked.
				let h = if confirm { 4000.0 } else { h };
				let mut phone = if desktop {
					Phone::desktop(w, h, language)
				} else {
					Phone::new(w, h, language)
				};
				let mut state = phone::busy_state();
				state.peers.push(stranger());
				phone.show_state(&state);
				phone.page(Page::Connect);
				if confirm {
					phone.click_label(tr_trust(language));
				}
				let drawn = phone.settle();
				let screen = phone.screen().expand(1.0);
				for r in drawn.widgets.iter().chain(&drawn.painted) {
					if r.min.x < screen.min.x || r.max.x > screen.max.x {
						bad.push(format!("{language:?} {w}x{h} confirm={confirm}: {r:?}"));
					}
				}
			}
		}
	}
	bad.dedup();
	assert!(bad.is_empty(), "{bad:#?}");
}

fn tr_trust(language: Language) -> &'static str {
	match language {
		Language::De => "Diesem Gerät vertrauen",
		Language::Ru => "Доверять этому устройству",
		_ => "Trust this device",
	}
}
