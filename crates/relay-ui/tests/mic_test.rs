//! The microphone test of Settings, Audio: the dots, the raw dB, and a microphone that is open
//! only while the test is on screen.

// The turn lock is held on purpose across awaits: the active language is process-wide.
#![allow(clippy::await_holding_lock)]

mod phone;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use phone::{Drawn, Phone};
use relay_ui::capabilities::SettingsCategory;
use relay_ui::engine::{Command, EngineError, Operations, Outcome};
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::mic_test::{DOTS, Ended, MicTest, Zone};
use relay_ui::model::Meters;

static TURN: Mutex<()> = Mutex::new(());

fn turn() -> std::sync::MutexGuard<'static, ()> {
	TURN.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
struct Ops {
	commands: Mutex<Vec<Command>>,
}

#[async_trait]
impl Operations for Ops {
	async fn run(&self, command: Command) -> Result<Outcome, EngineError> {
		self.commands.lock().unwrap().push(command);
		Ok(Outcome::default())
	}
}

impl Ops {
	fn seen(&self) -> Vec<Command> {
		self.commands.lock().unwrap().clone()
	}
}

fn rig() -> (Phone, Arc<Ops>) {
	let mut phone = Phone::new(360.0, 2600.0, Language::En);
	phone.show_state(&phone::busy_state());
	let ops = Arc::new(Ops::default());
	phone.slot.set(ops.clone());
	phone.actions.hub.set_visible(true);
	phone.app.open_settings(Some(SettingsCategory::Audio));
	phone.settle();
	(phone, ops)
}

fn level(phone: &Phone, seq: u64, dbfs: Option<f32>) {
	use relay_ui::emitter::Sink;
	relay_ui::actions::ShellSink {
		shared: phone.actions.shared.clone(),
		shell: phone.shell.clone(),
	}
	.meters(&Meters {
		seq,
		microphone_dbfs: dbfs,
		calls: Vec::new(),
	});
}

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
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

#[test]
fn the_dots_light_up_to_the_level_and_turn_yellow_then_red_near_clipping() {
	let mut test = MicTest::default();
	test.start(0.0);
	test.tick(0.1, Some(-30.0));
	let lit = (0..DOTS).filter(|i| test.lit(*i)).count();
	assert!((5..=8).contains(&lit), "{lit} dots at -30 dB");
	assert!((0..lit).all(|i| MicTest::zone(i) == Zone::Green));
	test.tick(0.2, Some(0.0));
	assert!((0..DOTS).all(|i| test.lit(i)), "full scale lights them all");
	assert_eq!(MicTest::zone(DOTS - 1), Zone::Red);
	assert_eq!(MicTest::zone(DOTS - 2), Zone::Red);
	assert_eq!(MicTest::zone(DOTS - 3), Zone::Yellow);
	assert_eq!(MicTest::zone(0), Zone::Green);
	test.tick(0.3, None);
	assert!((0..DOTS).all(|i| !test.lit(i)), "silence lights none");
}

#[test]
fn the_peak_is_held_and_then_falls() {
	let mut test = MicTest::default();
	test.start(0.0);
	test.tick(0.1, Some(-6.0));
	let high = test.peak_dot().unwrap();
	test.tick(0.5, Some(-40.0));
	assert_eq!(test.peak_dot(), Some(high), "held");
	test.tick(1.0, Some(-40.0));
	test.tick(1.6, Some(-40.0));
	test.tick(1.9, Some(-40.0));
	assert!(test.peak_dot().unwrap() < high, "released");
}

#[test]
fn a_test_nobody_looks_at_ends_by_itself() {
	let mut test = MicTest::default();
	test.start(0.0);
	assert_eq!(test.tick(0.05, Some(-20.0)), None);
	assert_eq!(test.tick(5.0, Some(-20.0)), Some(Ended::Abandoned));
	assert!(!test.running());
	test.start(10.0);
	let mut now = 10.0;
	while now < 41.0 {
		now += 0.5;
		if let Some(ended) = test.tick(now, None) {
			assert_eq!(ended, Ended::TimedOut);
			return;
		}
	}
	panic!("the engine's limit never reached");
}

#[tokio::test]
async fn the_card_is_called_microphone_test_and_the_old_no_signal_row_is_gone() {
	let _turn = turn();
	let (mut phone, _ops) = rig();
	let drawn = phone.settle();
	assert!(has(&drawn, "Microphone Test"), "{:?}", drawn.labels);
	assert!(has(&drawn, "Start test"));
	assert!(!has(&drawn, "Microphone level"));
	assert!(!has(&drawn, "no signal data"));
}

#[tokio::test]
async fn start_opens_the_microphone_and_shows_the_raw_db_next_to_the_dots() {
	let _turn = turn();
	let (mut phone, ops) = rig();
	phone.click_label("Start test");
	until(|| ops.seen().contains(&Command::StartMicTest)).await;
	level(&phone, 1, Some(-23.4));
	let drawn = phone.settle();
	assert!(has(&drawn, "Stop test"), "{:?}", drawn.labels);
	assert!(has(&drawn, "-23 dB"), "{:?}", drawn.labels);
	// Silence reads as minus infinity (the sign comes from the icon font).
	level(&phone, 2, None);
	let drawn = phone.settle();
	assert!(
		phone::has_icon(&drawn, relay_ui::icons::Icon::Infinity),
		"{:?}",
		drawn.labels
	);
	phone.click_label("Stop test");
	until(|| ops.seen().contains(&Command::StopMicTest)).await;
	assert!(has(&phone.settle(), "Start test"));
}

#[tokio::test]
async fn leaving_the_category_or_the_page_closes_the_microphone() {
	let _turn = turn();
	let (mut phone, ops) = rig();
	phone.click_label("Start test");
	until(|| ops.seen().contains(&Command::StartMicTest)).await;
	phone.app.open_settings(Some(SettingsCategory::Calls));
	phone.settle();
	until(|| ops.seen().contains(&Command::StopMicTest)).await;

	let (mut phone, ops) = rig();
	phone.click_label("Start test");
	until(|| ops.seen().contains(&Command::StartMicTest)).await;
	phone.page(Page::Contacts);
	until(|| ops.seen().contains(&Command::StopMicTest)).await;
}

#[tokio::test]
async fn a_hidden_window_closes_the_microphone() {
	let _turn = turn();
	let (mut phone, ops) = rig();
	phone.click_label("Start test");
	until(|| ops.seen().contains(&Command::StartMicTest)).await;
	phone
		.shell
		.visible
		.store(false, std::sync::atomic::Ordering::SeqCst);
	phone.settle();
	until(|| ops.seen().contains(&Command::StopMicTest)).await;
}

/// The repaint delay egui asks for after one more frame.
fn delay(phone: &mut Phone) -> Duration {
	let mut out = Default::default();
	let from = phone.ctx.input(|i| i.time);
	for i in 0..30 {
		let input = egui::RawInput {
			screen_rect: Some(phone.screen()),
			time: Some(from + 0.02 * f64::from(i + 1)),
			..Default::default()
		};
		let app = &mut phone.app;
		out = phone.ctx.run_ui(input, |ui| {
			app.begin_frame(ui.ctx());
			app.ui(ui);
		});
		out.textures_delta.clear();
	}
	out.viewport_output
		.get(&egui::ViewportId::ROOT)
		.map_or(Duration::MAX, |v| v.repaint_delay)
}

#[tokio::test]
async fn the_screen_repaints_only_while_the_test_runs() {
	let _turn = turn();
	let (mut phone, ops) = rig();
	assert_eq!(delay(&mut phone), Duration::MAX, "idle: no repaint at all");
	phone.click_label("Start test");
	until(|| ops.seen().contains(&Command::StartMicTest)).await;
	assert!(
		delay(&mut phone) <= Duration::from_millis(100),
		"running: follows the level"
	);
	phone.click_label("Stop test");
	assert_eq!(delay(&mut phone), Duration::MAX, "stopped: idle again");
}
