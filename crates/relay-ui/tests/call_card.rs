//! The card of a call in progress: the three-way microphone choice, the hold-to-talk button, the
//! connected time, the per-person volume popup and the hidden connection details.

mod phone;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use egui::{Event, Key, Modifiers, PointerButton};
use phone::stats::Ops;
use phone::{Drawn, Phone};
use relay_ui::engine::Command;
use relay_ui::i18n::Language;
use relay_ui::icons::Icon;
use relay_ui::layout::Page;
use relay_ui::model::{Mode, Phase};

/// 2026-11-05 12:00:00 UTC.
const NOW: u64 = 1_793_880_000_000;
const CALL: [u8; 16] = phone::stats::CALL;

fn rig_with(
	desktop: bool,
	mode: Mode,
	tweak: impl FnOnce(&mut relay_ui::model::AppState),
) -> (Phone, Arc<Ops>) {
	let mut phone = if desktop {
		Phone::desktop(720.0, 1600.0, Language::En)
	} else {
		Phone::new(360.0, 1600.0, Language::En)
	};
	phone.shell.now_ms.store(NOW, Ordering::SeqCst);
	let mut state = phone::busy_state();
	state.calls.retain(|c| c.call_id == "02".repeat(16));
	let call = &mut state.calls[0];
	call.connected_at_unix_ms = NOW - 75_000;
	call.transmit.mode = mode;
	call.transmit.consent_granted = mode != Mode::Muted;
	tweak(&mut state);
	phone.show_state(&state);
	phone.actions.hub.update(|s| *s = state.clone());
	let ops = Arc::new(Ops::default());
	phone.slot.set(ops.clone());
	phone.page(Page::Calls);
	(phone, ops)
}

fn rig(mode: Mode) -> (Phone, Arc<Ops>) {
	rig_with(false, mode, |_| {})
}

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

async fn until(mut done: impl FnMut() -> bool) {
	for _ in 0..200 {
		if done() {
			return;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!("condition not reached in time");
}

fn glyph(icon: Icon) -> String {
	icon.codepoint().to_string()
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
async fn there_is_no_consent_checkbox_and_the_active_mode_is_the_only_one_filled() {
	let (mut phone, _ops) = rig(Mode::Muted);
	let drawn = phone.settle();
	assert!(
		!drawn
			.labels
			.iter()
			.any(|l| l.contains("I allow my microphone")),
		"{:?}",
		drawn.labels
	);
	for label in ["Muted", "Unmuted", "Push to talk"] {
		assert!(has(&drawn, label), "{label}: {:?}", drawn.labels);
	}
}

#[tokio::test]
async fn each_mode_button_sends_its_own_mode() {
	let (mut phone, ops) = rig(Mode::Muted);
	phone.click_label("Unmuted");
	until(|| !ops.sent().is_empty()).await;
	phone.click_label("Push to talk");
	until(|| ops.sent().len() == 2).await;
	let modes: Vec<_> = ops
		.sent()
		.into_iter()
		.filter_map(|c| match c {
			Command::SetTransmit { call_id, mode, .. } => {
				assert_eq!(call_id, CALL);
				Some(mode)
			}
			_ => None,
		})
		.collect();
	assert_eq!(modes, [Mode::Unmuted, Mode::PushToTalk]);
	// Pressing the mode the call is already in sends nothing.
	let (mut phone, ops) = rig(Mode::Unmuted);
	phone.click_label("Unmuted");
	tokio::time::sleep(Duration::from_millis(60)).await;
	assert!(ops.sent().is_empty(), "{:?}", ops.sent());
	phone.click_label("Muted");
	until(|| !ops.sent().is_empty()).await;
	assert!(matches!(
		ops.sent()[0],
		Command::SetTransmit {
			mode: Mode::Muted,
			..
		}
	));
}

fn hold_button(drawn: &Drawn) -> egui::Rect {
	*drawn
		.tappable
		.iter()
		.find(|r| r.height() >= 70.0 && r.width() >= 200.0)
		.expect("the hold-to-talk button")
}

fn pointer(phone: &mut Phone, pos: egui::Pos2, pressed: bool) {
	phone.frame_with(vec![Event::PointerMoved(pos)]);
	phone.frame_with(vec![Event::PointerButton {
		pos,
		button: PointerButton::Primary,
		pressed,
		modifiers: Default::default(),
	}]);
	phone.frame_with(vec![]);
}

#[tokio::test]
async fn the_hold_button_follows_the_mouse_on_desktop_too_and_only_in_push_to_talk() {
	let (mut phone, _ops) = rig_with(true, Mode::Unmuted, |_| {});
	let drawn = phone.settle();
	assert!(
		!drawn
			.tappable
			.iter()
			.any(|r| r.height() >= 70.0 && r.width() >= 200.0),
		"no talk button while unmuted"
	);
	let (mut phone, _ops) = rig_with(true, Mode::PushToTalk, |_| {});
	let drawn = phone.settle();
	let at = hold_button(&drawn).center();
	pointer(&mut phone, at, true);
	assert_eq!(*phone.shell.ptt.lock().unwrap(), [("02".repeat(16), true)]);
	pointer(&mut phone, at, false);
	assert_eq!(
		*phone.shell.ptt.lock().unwrap(),
		[("02".repeat(16), true), ("02".repeat(16), false)]
	);
}

#[tokio::test]
async fn a_held_button_is_released_when_the_call_ends_or_the_mode_changes() {
	for end in [true, false] {
		let (mut phone, _ops) = rig(Mode::PushToTalk);
		let at = hold_button(&phone.settle()).center();
		pointer(&mut phone, at, true);
		let mut state = (*phone.actions.shared.state()).clone();
		if end {
			state.calls[0].phase = Phase::Ended;
		} else {
			state.calls[0].transmit.mode = Mode::Unmuted;
		}
		phone.show_state(&state);
		phone.settle();
		let log = phone.shell.ptt.lock().unwrap().clone();
		assert_eq!(log.last().map(|e| e.1), Some(false), "end={end}: {log:?}");
	}
}

#[tokio::test]
async fn the_connected_time_counts_from_the_engine_clock_and_ticks_each_second() {
	let (mut phone, _ops) = rig(Mode::Muted);
	assert!(has(&phone.settle(), "Connected \u{b7} 1:15"));
	phone.shell.now_ms.store(NOW + 3_525_000, Ordering::SeqCst);
	assert!(has(&phone.settle(), "Connected \u{b7} 1:00:00"));
	// A call whose start the engine did not report shows no time.
	let (mut phone, _ops) = rig_with(false, Mode::Muted, |s| s.calls[0].connected_at_unix_ms = 0);
	assert!(has(&phone.settle(), "Connected"));
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
async fn a_visible_connected_call_repaints_once_a_second_and_no_call_never() {
	let (mut phone, _ops) = rig(Mode::Muted);
	phone.settle();
	assert_eq!(repaint_delay(&mut phone), Duration::from_secs(1));
	let (mut phone, _ops) = rig_with(true, Mode::Muted, |s| s.calls.clear());
	phone.settle();
	assert_eq!(repaint_delay(&mut phone), Duration::MAX, "idle");
}

#[tokio::test]
async fn the_volume_popup_opens_on_a_right_click_and_resets() {
	let (mut phone, ops) = rig_with(true, Mode::Muted, |s| s.calls[0].receive_gain = 0.5);
	let drawn = phone.settle();
	assert!(!has(&drawn, "Reset to 100%"), "hidden until asked");
	assert!(!drawn.labels.iter().any(|l| l.contains("Their volume")));
	let name = drawn
		.named
		.iter()
		.find(|(t, _)| t == "Ada Lovelace")
		.unwrap()
		.1;
	secondary_click(&mut phone, name.center());
	let open = phone.settle();
	assert!(has(&open, "Ada Lovelace's volume"), "{:?}", open.labels);
	assert!(has(&open, "50%"), "{:?}", open.labels);
	phone.click_label("Reset to 100%");
	until(|| !ops.sent().is_empty()).await;
	match &ops.sent()[0] {
		Command::SetCallControls { receive_gain, .. } => assert_eq!(*receive_gain, Some(1.0)),
		other => panic!("{other:?}"),
	}
}

#[tokio::test]
async fn the_volume_popup_is_also_in_the_menu() {
	let (mut phone, _ops) = rig(Mode::Muted);
	phone.click_label(&glyph(Icon::More));
	phone.click_label("Volume");
	let open = phone.settle();
	assert!(has(&open, "Ada Lovelace's volume"), "{:?}", open.labels);
}

#[tokio::test]
async fn the_details_panel_opens_the_stats_stream_only_while_it_is_shown() {
	let (mut phone, ops) = rig(Mode::Muted);
	let drawn = phone.settle();
	assert!(!has(&drawn, "Round trip"));
	assert_eq!(
		ops.opened.load(Ordering::SeqCst),
		0,
		"closed panel, no stream"
	);
	phone.click_label(&glyph(Icon::More));
	phone.click_label("Connection details");
	let call = "02".repeat(16);
	until(|| phone.actions.shared.stats.latest(&call).is_some()).await;
	let panel = phone.settle();
	assert_eq!(ops.opened.load(Ordering::SeqCst), 1);
	for (label, value) in [
		("Round trip", "41 ms (best 38 ms, worst 52 ms)"),
		("Jitter", "3 ms"),
		("Lost packets", "0.4% now \u{b7} 0.1% overall"),
		("Jitter buffer", "36 ms filled of 40 ms"),
		("Delay mouth to ear", "about 120 ms (estimate)"),
		("Bitrate", "32 kbit/s"),
		("Connection", "Not working: no reply to the path challenge"),
	] {
		assert!(
			has(&panel, label) && has(&panel, value),
			"{label}: {:?}",
			panel.labels
		);
	}
	// The addresses and clock drift are one level deeper.
	assert!(!has(&panel, "Clock drift"));
	phone.click_label("Details");
	let more = phone.settle();
	assert!(
		has(&more, "Clock drift") && has(&more, "+12 ppm"),
		"{:?}",
		more.labels
	);
	assert!(has(&more, "10.0.0.2:5000 \u{2192} 10.0.0.3:5000"));
	// Hiding it closes the stream.
	phone.click_label(&glyph(Icon::More));
	phone.click_label("Hide connection details");
	phone.settle();
	until(|| ops.closed.load(Ordering::SeqCst) == 1).await;
	assert!(!has(&phone.settle(), "Round trip"));
}

#[tokio::test]
async fn the_shortcut_toggles_the_panel_and_the_call_ending_closes_the_stream() {
	let (mut phone, ops) = rig_with(true, Mode::Muted, |_| {});
	let shortcut = || Event::Key {
		key: Key::D,
		physical_key: None,
		pressed: true,
		repeat: false,
		modifiers: Modifiers::COMMAND | Modifiers::SHIFT,
	};
	phone.frame_with(vec![shortcut()]);
	phone.settle();
	until(|| ops.opened.load(Ordering::SeqCst) == 1).await;
	// The call ends: no card, no panel, no stream.
	let mut state = (*phone.actions.shared.state()).clone();
	state.calls[0].phase = Phase::Ended;
	phone.show_state(&state);
	phone.settle();
	until(|| ops.closed.load(Ordering::SeqCst) == 1).await;
	assert_eq!(phone.actions.shared.stats.watching(), None);
}

#[tokio::test]
async fn the_incoming_prompt_says_the_mode_the_call_will_start_in() {
	for (mode, line) in [
		(Mode::PushToTalk, "You will join in push-to-talk mode."),
		(Mode::Unmuted, "You will join with your microphone on."),
		(Mode::Muted, "You will join with your microphone off."),
	] {
		let mut phone = Phone::new(360.0, 1000.0, Language::En);
		let mut state = phone::busy_state();
		state.settings.as_mut().unwrap().default_transmit_mode = mode;
		phone.show_state(&state);
		let drawn = phone.page(Page::Calls);
		assert!(has(&drawn, line), "{line}: {:?}", drawn.labels);
		assert!(has(&drawn, "Accept") && has(&drawn, "Decline"));
		assert!(!drawn.labels.iter().any(|l| l.contains("does not turn on")));
	}
}
