//! Shared rig for the phone-size screen tests: a window-less egui context at a
//! phone's width and density with a fake shell whose insets can be set.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use relay_ui::actions::{Actions, Shared, ShellSink};
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::emitter::Sink;
use relay_ui::hub::Hub;
use relay_ui::i18n::{self, Language};
use relay_ui::layout::Page;
use relay_ui::mock::seed;
use relay_ui::model::AppState;
use relay_ui::qr_scan::ScanError;
use relay_ui::shell::{BoxFuture, FilePicker, Insets, PickedFile, Shell};
use relay_ui::ui::RelayApp;

/// Pixels per point of a typical phone.
pub const DENSITY: f32 = 2.6;
/// Phone sizes in points: (width, height).
pub const SIZES: [(f32, f32); 2] = [(360.0, 800.0), (320.0, 640.0)];
/// Tall viewports of the same widths, so a whole page is on screen at once.
pub const TALL: [(f32, f32); 2] = [(360.0, 5000.0), (320.0, 5000.0)];
/// Languages with the longest strings.
pub const LANGUAGES: [Language; 3] = [Language::En, Language::De, Language::Ru];

#[derive(Default)]
pub struct PhoneShell {
	pub insets: Mutex<Insets>,
	pub keyboard: AtomicU32,
	pub ptt: Mutex<Vec<(String, bool)>>,
	/// Everything handed to `copy_text`.
	pub copied: Mutex<Vec<String>>,
	visible: AtomicBool,
	/// Behave like a desktop shell: closing hides to a tray, no talk button.
	pub desktop: AtomicBool,
	/// The time the shell reports, in unix milliseconds; 0 is the system clock.
	pub now_ms: AtomicU64,
	/// `can_scan_qr`: the shell has a camera scanner.
	pub scanner: AtomicBool,
	/// How many times the UI asked for a scan, and the answer each one gets.
	pub scans: AtomicUsize,
	pub scan_reply: Mutex<Option<Result<Option<String>, ScanError>>>,
	pub camera_settings: AtomicUsize,
}

impl FilePicker for PhoneShell {
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async { Ok(None) })
	}
}

impl Shell for PhoneShell {
	fn copy_text(&self, text: &str) {
		self.copied.lock().unwrap().push(text.to_owned());
	}
	fn now_unix_ms(&self) -> u64 {
		match self.now_ms.load(Ordering::SeqCst) {
			0 => std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.map_or(0, |d| d.as_millis() as u64),
			fixed => fixed,
		}
	}
	fn time_zone(&self) -> relay_io::localtime::LocalZone {
		relay_io::localtime::LocalZone::utc()
	}
	fn request_repaint(&self) {}
	fn window_visible(&self) -> bool {
		self.visible.load(Ordering::SeqCst)
	}
	fn show_window(&self) {}
	fn hide_window(&self) {}
	fn quit(&self) {}
	fn hides_to_tray(&self) -> bool {
		self.desktop.load(Ordering::SeqCst)
	}
	fn runs_in_background(&self) -> bool {
		!self.desktop.load(Ordering::SeqCst)
	}
	fn safe_area_insets(&self) -> Insets {
		*self.insets.lock().unwrap()
	}
	fn keyboard_height(&self) -> f32 {
		f32::from_bits(self.keyboard.load(Ordering::SeqCst))
	}
	fn has_global_hotkey(&self) -> bool {
		false
	}
	fn has_talk_button(&self) -> bool {
		!self.desktop.load(Ordering::SeqCst)
	}
	fn push_to_talk(&self, call_id: &str, pressed: bool) {
		self.ptt.lock().unwrap().push((call_id.to_owned(), pressed));
	}
	fn can_scan_qr(&self) -> bool {
		self.scanner.load(Ordering::SeqCst)
	}
	fn scan_qr(&self) -> BoxFuture<'_, Result<Option<String>, ScanError>> {
		self.scans.fetch_add(1, Ordering::SeqCst);
		let reply = self.scan_reply.lock().unwrap().clone();
		Box::pin(async move { reply.unwrap_or(Ok(None)) })
	}
	fn open_camera_settings(&self) {
		self.camera_settings.fetch_add(1, Ordering::SeqCst);
	}
}

/// The mock state plus a connected push-to-talk call, so both an incoming and
/// a running call card are drawn.
pub fn busy_state() -> AppState {
	let mut state = seed();
	let mut running = state.calls[0].clone();
	running.call_id = "02".repeat(16);
	running.phase = relay_ui::model::Phase::Connected;
	running.transmit.mode = relay_ui::model::Mode::PushToTalk;
	state.calls.push(running);
	state
}

/// What one frame drew.
pub struct Drawn {
	/// Every widget (and container) rect of the frame.
	pub widgets: Vec<egui::Rect>,
	/// Enabled widgets that react to a tap and are at least partly on screen.
	pub tappable: Vec<egui::Rect>,
	/// The same widgets, cut to what is visible.
	pub visible: Vec<egui::Rect>,
	/// Bounds of everything painted.
	pub painted: Vec<egui::Rect>,
	/// Painted text: its visible bounds and its number of rows.
	pub texts: Vec<(egui::Rect, usize)>,
	/// The strings of all painted text.
	pub labels: Vec<String>,
	/// Each painted string with its visible bounds.
	pub named: Vec<(String, egui::Rect)>,
}

pub struct Phone {
	pub ctx: egui::Context,
	pub app: RelayApp,
	pub shell: Arc<PhoneShell>,
	pub actions: Actions,
	pub slot: Arc<SessionSlot>,
	pub size: egui::Vec2,
}

impl Phone {
	pub fn new(width: f32, height: f32, language: Language) -> Self {
		i18n::set_active(language);
		let slot = Arc::new(SessionSlot::default());
		let hub = Arc::new(Hub::default());
		hub.update(|s| *s = seed());
		let shell = Arc::new(PhoneShell::default());
		shell.visible.store(true, Ordering::SeqCst);
		let actions = Actions {
			controller: Arc::new(Controller::new(hub.clone(), slot.clone())),
			hub,
			shared: Arc::new(Shared::default()),
			runtime: tokio::runtime::Handle::current(),
			shell: shell.clone(),
		};
		let sink = ShellSink {
			shared: actions.shared.clone(),
			shell: shell.clone(),
		};
		sink.state(&busy_state());
		let ctx = egui::Context::default();
		ctx.set_pixels_per_point(DENSITY);
		let app = RelayApp::new(actions.clone(), &ctx);
		Self {
			ctx,
			app,
			shell,
			actions,
			slot,
			size: egui::vec2(width, height),
		}
	}

	/// Replace the state the screen shows.
	pub fn show_state(&self, state: &AppState) {
		ShellSink {
			shared: self.actions.shared.clone(),
			shell: self.shell.clone(),
		}
		.state(state);
	}

	/// A rig sized like a desktop window, with a desktop shell.
	pub fn desktop(width: f32, height: f32, language: Language) -> Self {
		let phone = Self::new(width, height, language);
		phone.shell.desktop.store(true, Ordering::SeqCst);
		phone
	}

	pub fn screen(&self) -> egui::Rect {
		egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)
	}

	pub fn frame_with(&mut self, events: Vec<egui::Event>) -> Drawn {
		let input = egui::RawInput {
			screen_rect: Some(self.screen()),
			events,
			..Default::default()
		};
		let app = &mut self.app;
		let mut out = self.ctx.run_ui(input, |ui| {
			app.begin_frame(ui.ctx());
			app.ui(ui);
		});
		out.textures_delta.clear();
		let painted = out
			.shapes
			.iter()
			.map(|s| s.shape.visual_bounding_rect())
			.filter(|r| r.is_finite())
			.collect();
		let texts = out
			.shapes
			.iter()
			.filter_map(|s| match &s.shape {
				egui::Shape::Text(t) => Some((
					t.visual_bounding_rect().intersect(s.clip_rect),
					t.galley.rows.len(),
				)),
				_ => None,
			})
			.collect();
		let labels = out
			.shapes
			.iter()
			.filter_map(|s| match &s.shape {
				egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
				_ => None,
			})
			.collect();
		let named = out
			.shapes
			.iter()
			.filter_map(|s| match &s.shape {
				egui::Shape::Text(t) => Some((
					t.galley.text().to_owned(),
					t.visual_bounding_rect().intersect(s.clip_rect),
				)),
				_ => None,
			})
			.collect();
		let mut widgets = Vec::new();
		let mut tappable = Vec::new();
		let mut visible = Vec::new();
		self.ctx.viewport(|v| {
			for (_, list) in v.prev_pass.widgets.layers() {
				for w in list {
					widgets.push(w.rect);
					let seen = w.interact_rect.width() > 0.0 && w.interact_rect.height() > 0.0;
					if w.enabled && w.sense.senses_click() && seen {
						tappable.push(w.rect);
						visible.push(w.interact_rect);
					}
				}
			}
		});
		Drawn {
			widgets,
			tappable,
			visible,
			painted,
			texts,
			labels,
			named,
		}
	}

	/// Draw until the layout settles (a few frames) and report the last one.
	/// The rects of the bottom navigation tabs, left to right: the controls
	/// whose bottom edge rests on the bottom inset.
	pub fn tabs(&self, drawn: &Drawn) -> Vec<egui::Rect> {
		let floor = self.size.y - self.shell.insets.lock().unwrap().bottom;
		let mut tabs: Vec<_> = drawn
			.tappable
			.iter()
			.copied()
			.filter(|r| {
				(floor - 12.0..=floor + 1.0).contains(&r.max.y) && r.width() < self.size.x / 2.0
			})
			.collect();
		tabs.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
		tabs
	}

	pub fn settle(&mut self) -> Drawn {
		for _ in 0..4 {
			self.frame_with(Vec::new());
		}
		self.frame_with(Vec::new())
	}

	/// Click the first painted text that equals `label`.
	pub fn click_label(&mut self, label: &str) {
		let drawn = self.settle();
		let (_, rect) = drawn
			.named
			.iter()
			.find(|(t, _)| t == label)
			.unwrap_or_else(|| panic!("no {label:?} in {:?}", drawn.labels));
		let pos = rect.center();
		let button = |pressed| egui::Event::PointerButton {
			pos,
			button: egui::PointerButton::Primary,
			pressed,
			modifiers: Default::default(),
		};
		self.frame_with(vec![egui::Event::PointerMoved(pos)]);
		self.frame_with(vec![button(true)]);
		self.frame_with(vec![button(false)]);
		self.settle();
	}

	pub fn page(&mut self, page: Page) -> Drawn {
		self.app.set_page(page);
		self.settle()
	}
}
