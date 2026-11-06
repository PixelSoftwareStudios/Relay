//! The microphone card on the Calls page and the pure state behind it.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use relay_ui::actions::{Actions, Shared};
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::hub::Hub;
use relay_ui::microphone::{Dismissal, MicrophoneAccess, MicrophoneCard, card};
use relay_ui::shell::{BoxFuture, FilePicker, PickedFile, Shell};
use relay_ui::ui::RelayApp;

use MicrophoneAccess::{Blocked, Denied, NotAsked, Ready};

#[test]
fn only_a_missing_permission_shows_a_card() {
	assert_eq!(card(Ready, false), None);
	assert_eq!(card(NotAsked, false), Some(MicrophoneCard::Ask));
	assert_eq!(card(Denied, false), Some(MicrophoneCard::Ask));
	assert_eq!(card(Blocked, false), Some(MicrophoneCard::TurnedOff));
	assert_eq!(card(Blocked, true), None);
}

#[test]
fn a_dismissal_lasts_until_the_access_changes() {
	let mut d = Dismissal::default();
	assert!(!d.is_dismissed(NotAsked));
	d.dismiss();
	assert!(d.is_dismissed(NotAsked));
	// Refused for good: a different situation, so the card returns.
	assert!(!d.is_dismissed(Blocked));
	d.dismiss();
	assert!(d.is_dismissed(Blocked));
	// Granted then revoked later: shown again.
	assert!(!d.is_dismissed(Ready));
	assert!(!d.is_dismissed(Blocked));
}

#[derive(Default)]
struct FakeShell {
	access: AtomicU8,
	asked: AtomicUsize,
	settings: AtomicUsize,
	copied: Mutex<Vec<String>>,
}

impl FakeShell {
	fn set(&self, access: MicrophoneAccess) {
		self.access.store(access as u8, Ordering::SeqCst);
	}
}

impl FilePicker for FakeShell {
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async { Ok(None) })
	}
}

impl Shell for FakeShell {
	fn copy_text(&self, text: &str) {
		self.copied.lock().unwrap().push(text.to_owned());
	}
	fn request_repaint(&self) {}
	fn window_visible(&self) -> bool {
		true
	}
	fn show_window(&self) {}
	fn hide_window(&self) {}
	fn quit(&self) {}
	fn hides_to_tray(&self) -> bool {
		false
	}
	fn microphone_access(&self) -> MicrophoneAccess {
		match self.access.load(Ordering::SeqCst) {
			1 => NotAsked,
			2 => Denied,
			3 => Blocked,
			_ => Ready,
		}
	}
	fn request_microphone(&self) {
		self.asked.fetch_add(1, Ordering::SeqCst);
	}
	fn open_microphone_settings(&self) {
		self.settings.fetch_add(1, Ordering::SeqCst);
	}
}

struct Rig {
	ctx: egui::Context,
	app: RelayApp,
	shell: Arc<FakeShell>,
}

fn rig() -> Rig {
	let hub = Arc::new(Hub::default());
	let shell = Arc::new(FakeShell::default());
	let actions = Actions {
		controller: Arc::new(Controller::new(
			hub.clone(),
			Arc::new(SessionSlot::default()),
		)),
		hub,
		shared: Arc::new(Shared::default()),
		runtime: tokio::runtime::Handle::current(),
		shell: shell.clone(),
	};
	let ctx = egui::Context::default();
	let app = RelayApp::new(actions, &ctx);
	Rig { ctx, app, shell }
}

fn texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
	match shape {
		egui::Shape::Text(t) => out.push((t.galley.text().to_owned(), t.pos)),
		egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
		_ => {}
	}
}

impl Rig {
	fn frame(&mut self, events: Vec<egui::Event>) -> Vec<(String, egui::Pos2)> {
		let input = egui::RawInput {
			screen_rect: Some(egui::Rect::from_min_size(
				egui::Pos2::ZERO,
				egui::vec2(390.0, 800.0),
			)),
			events,
			..Default::default()
		};
		let app = &mut self.app;
		let mut out = self.ctx.run_ui(input, |ui| {
			app.begin_frame(ui.ctx());
			app.ui(ui);
		});
		out.textures_delta.clear();
		let mut found = Vec::new();
		out.shapes.iter().for_each(|s| texts(&s.shape, &mut found));
		found
	}

	fn click(&mut self, label: &str) {
		// The first frame is a sizing pass: positions settle on the second.
		self.frame(vec![]);
		let at = self
			.frame(vec![])
			.into_iter()
			.find(|(text, _)| text == label)
			.unwrap_or_else(|| panic!("no {label:?} on screen"))
			.1 + egui::vec2(4.0, 4.0);
		let button = egui::PointerButton::Primary;
		self.frame(vec![egui::Event::PointerMoved(at)]);
		let press = |pressed| egui::Event::PointerButton {
			pos: at,
			button,
			pressed,
			modifiers: egui::Modifiers::NONE,
		};
		self.frame(vec![press(true)]);
		self.frame(vec![press(false)]);
	}
}

fn has(found: &[(String, egui::Pos2)], text: &str) -> bool {
	found.iter().any(|(t, _)| t == text)
}

#[tokio::test]
async fn a_platform_that_needs_nothing_shows_no_card() {
	let mut r = rig();
	let found = r.frame(vec![]);
	assert!(!has(&found, relay_ui::tr!("mic-ask-title")));
	assert!(!has(&found, relay_ui::tr!("mic-off-title")));
}

#[tokio::test]
async fn not_asked_shows_the_ask_card_and_the_button_prompts() {
	let mut r = rig();
	r.shell.set(NotAsked);
	assert!(has(&r.frame(vec![]), relay_ui::tr!("mic-ask-title")));
	r.click(relay_ui::tr!("mic-allow"));
	assert_eq!(r.shell.asked.load(Ordering::SeqCst), 1);
	// Granted: the card goes away without a restart.
	r.shell.set(Ready);
	assert!(!has(&r.frame(vec![]), relay_ui::tr!("mic-ask-title")));
}

#[tokio::test]
async fn blocked_explains_and_opens_the_settings() {
	let mut r = rig();
	r.shell.set(Blocked);
	let found = r.frame(vec![]);
	assert!(has(&found, relay_ui::tr!("mic-off-title")));
	r.click(relay_ui::tr!("mic-open-settings"));
	assert_eq!(r.shell.settings.load(Ordering::SeqCst), 1);
	assert_eq!(r.shell.asked.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn dismissing_hides_the_card() {
	let mut r = rig();
	r.shell.set(Denied);
	r.click(relay_ui::tr!("action-dismiss"));
	assert!(!has(&r.frame(vec![]), relay_ui::tr!("mic-ask-title")));
}
