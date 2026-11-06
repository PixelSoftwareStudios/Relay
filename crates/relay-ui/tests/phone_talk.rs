//! The on-screen talk button follows touch press and release, is large, and is
//! released when it disappears.

mod phone;

use phone::Phone;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;

fn touch(phone: &mut Phone, pos: egui::Pos2, phase: egui::TouchPhase) {
	let events = vec![
		egui::Event::Touch {
			device_id: egui::TouchDeviceId(0),
			id: egui::TouchId(1),
			phase,
			pos,
			force: None,
		},
		match phase {
			egui::TouchPhase::Start => egui::Event::PointerButton {
				pos,
				button: egui::PointerButton::Primary,
				pressed: true,
				modifiers: Default::default(),
			},
			_ => egui::Event::PointerButton {
				pos,
				button: egui::PointerButton::Primary,
				pressed: false,
				modifiers: Default::default(),
			},
		},
	];
	phone.frame_with(vec![egui::Event::PointerMoved(pos)]);
	phone.frame_with(events);
}

#[tokio::test]
async fn the_talk_button_is_large_and_reports_press_and_release() {
	let mut phone = Phone::new(360.0, 3000.0, Language::En);
	let drawn = phone.page(Page::Calls);
	let big: Vec<_> = drawn
		.tappable
		.iter()
		.filter(|r| r.height() >= 70.0 && r.width() >= 200.0)
		.collect();
	assert!(
		!big.is_empty(),
		"no large talk button on a push-to-talk call"
	);
	let target = big[0].center();
	touch(&mut phone, target, egui::TouchPhase::Start);
	phone.frame_with(Vec::new());
	assert_eq!(phone.shell.ptt.lock().unwrap().len(), 1);
	assert!(phone.shell.ptt.lock().unwrap()[0].1, "press reported");
	touch(&mut phone, target, egui::TouchPhase::End);
	phone.frame_with(Vec::new());
	let log = phone.shell.ptt.lock().unwrap().clone();
	assert_eq!(log.len(), 2, "{log:?}");
	assert!(!log[1].1, "release reported");
	assert_eq!(log[0].0, log[1].0);
}

#[tokio::test]
async fn leaving_the_page_while_held_releases() {
	let mut phone = Phone::new(360.0, 3000.0, Language::En);
	let drawn = phone.page(Page::Calls);
	let target = drawn
		.tappable
		.iter()
		.find(|r| r.height() >= 70.0 && r.width() >= 200.0)
		.expect("talk button")
		.center();
	touch(&mut phone, target, egui::TouchPhase::Start);
	phone.frame_with(Vec::new());
	phone.page(Page::People);
	let log = phone.shell.ptt.lock().unwrap().clone();
	assert_eq!(log.last().map(|e| e.1), Some(false), "{log:?}");
}
