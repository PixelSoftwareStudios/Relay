//! Shared rig for the phone-size screen tests: a window-less egui context at a
//! phone's width and density with a fake shell whose insets can be set.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use relay_ui::actions::{Actions, Shared};
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::hub::Hub;
use relay_ui::i18n::{self, Language};
use relay_ui::layout::Page;
use relay_ui::mock::seed;
use relay_ui::shell::{BoxFuture, FilePicker, Insets, PickedFile, Shell};
use relay_ui::ui::RelayApp;

/// Pixels per point of a typical phone.
pub const DENSITY: f32 = 2.6;
/// Phone sizes in points: (width, height).
pub const SIZES: [(f32, f32); 2] = [(360.0, 800.0), (320.0, 640.0)];
/// Languages with the longest strings.
pub const LANGUAGES: [Language; 3] = [Language::En, Language::De, Language::Ru];

#[derive(Default)]
pub struct PhoneShell {
	pub insets: Mutex<Insets>,
	pub keyboard: AtomicU32,
	pub ptt: Mutex<Vec<(String, bool)>>,
	visible: AtomicBool,
}

impl FilePicker for PhoneShell {
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async { Ok(None) })
	}
}

impl Shell for PhoneShell {
	fn copy_text(&self, _: &str) {}
	fn request_repaint(&self) {}
	fn window_visible(&self) -> bool {
		self.visible.load(Ordering::SeqCst)
	}
	fn show_window(&self) {}
	fn hide_window(&self) {}
	fn quit(&self) {}
	fn hides_to_tray(&self) -> bool {
		false
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
		true
	}
	fn push_to_talk(&self, call_id: &str, pressed: bool) {
		self.ptt.lock().unwrap().push((call_id.to_owned(), pressed));
	}
}

/// What one frame drew.
pub struct Drawn {
	/// Every widget (and container) rect of the frame.
	pub widgets: Vec<egui::Rect>,
	/// Enabled widgets that react to a tap.
	pub tappable: Vec<egui::Rect>,
	/// Bounds of everything painted.
	pub painted: Vec<egui::Rect>,
}

pub struct Phone {
	pub ctx: egui::Context,
	pub app: RelayApp,
	pub shell: Arc<PhoneShell>,
	pub actions: Actions,
	pub size: egui::Vec2,
}

impl Phone {
	pub fn new(width: f32, height: f32, language: Language) -> Self {
		i18n::set_active(language);
		let hub = Arc::new(Hub::default());
		hub.update(|s| *s = seed());
		let shell = Arc::new(PhoneShell::default());
		shell.visible.store(true, Ordering::SeqCst);
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
		ctx.set_pixels_per_point(DENSITY);
		let app = RelayApp::new(actions.clone(), &ctx);
		Self {
			ctx,
			app,
			shell,
			actions,
			size: egui::vec2(width, height),
		}
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
		let mut widgets = Vec::new();
		let mut tappable = Vec::new();
		self.ctx.viewport(|v| {
			for (_, list) in v.prev_pass.widgets.layers() {
				for w in list {
					widgets.push(w.rect);
					if w.enabled && w.sense.senses_click() {
						tappable.push(w.rect);
					}
				}
			}
		});
		Drawn {
			widgets,
			tappable,
			painted,
		}
	}

	/// Draw until the layout settles (a few frames) and report the last one.
	pub fn settle(&mut self) -> Drawn {
		for _ in 0..4 {
			self.frame_with(Vec::new());
		}
		self.frame_with(Vec::new())
	}

	pub fn page(&mut self, page: Page) -> Drawn {
		self.app.set_page(page);
		self.settle()
	}
}
