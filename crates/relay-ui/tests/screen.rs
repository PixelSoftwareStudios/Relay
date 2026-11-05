//! Window-less frames of the whole screen with a fake shell.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use relay_ui::actions::{Actions, Shared};
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::hub::Hub;
use relay_ui::layout::{LayoutClass, Page};
use relay_ui::mock::seed;
use relay_ui::shell::{BoxFuture, FilePicker, PickedFile, Shell};
use relay_ui::ui::RelayApp;

#[derive(Default)]
struct FakeShell {
	visible: AtomicBool,
	repaints: AtomicUsize,
	quits: AtomicUsize,
	hides: AtomicUsize,
	copied: std::sync::Mutex<Vec<String>>,
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
	fn request_repaint(&self) {
		self.repaints.fetch_add(1, Ordering::SeqCst);
	}
	fn window_visible(&self) -> bool {
		self.visible.load(Ordering::SeqCst)
	}
	fn show_window(&self) {}
	fn hide_window(&self) {
		self.hides.fetch_add(1, Ordering::SeqCst);
	}
	fn quit(&self) {
		self.quits.fetch_add(1, Ordering::SeqCst);
	}
	fn hides_to_tray(&self) -> bool {
		false
	}
}

struct Rig {
	ctx: egui::Context,
	app: RelayApp,
	shell: Arc<FakeShell>,
	hub: Arc<Hub>,
	actions: Actions,
}

fn rig() -> Rig {
	let hub = Arc::new(Hub::default());
	hub.update(|s| *s = seed());
	let shell = Arc::new(FakeShell::default());
	shell.visible.store(true, Ordering::SeqCst);
	let actions = Actions {
		controller: Arc::new(Controller::new(
			hub.clone(),
			Arc::new(SessionSlot::default()),
		)),
		hub: hub.clone(),
		shared: Arc::new(Shared::default()),
		runtime: tokio::runtime::Handle::current(),
		shell: shell.clone(),
	};
	let ctx = egui::Context::default();
	let app = RelayApp::new(actions.clone(), &ctx);
	Rig {
		ctx,
		app,
		shell,
		hub,
		actions,
	}
}

impl Rig {
	/// One frame of a window this wide; returns the repaint delay it asked for.
	fn frame(&mut self, width: f32) -> Duration {
		let input = egui::RawInput {
			screen_rect: Some(egui::Rect::from_min_size(
				egui::Pos2::ZERO,
				egui::vec2(width, 800.0),
			)),
			..Default::default()
		};
		let app = &mut self.app;
		let mut out = self.ctx.run_ui(input, |ui| {
			app.begin_frame(ui.ctx());
			app.ui(ui);
		});
		out.textures_delta.clear();
		out.viewport_output
			.get(&egui::ViewportId::ROOT)
			.map_or(Duration::MAX, |v| v.repaint_delay)
	}
}

#[tokio::test]
async fn width_selects_the_layout() {
	let mut r = rig();
	r.frame(390.0);
	assert_eq!(r.app.layout_class(), Some(LayoutClass::Compact));
	r.frame(900.0);
	assert_eq!(r.app.layout_class(), Some(LayoutClass::Regular));
}

#[tokio::test]
async fn every_compact_page_draws() {
	let mut r = rig();
	for page in Page::ALL {
		r.app.set_page(page);
		for _ in 0..3 {
			r.frame(390.0);
		}
		assert_eq!(r.app.page(), page);
	}
}

#[tokio::test]
async fn an_idle_screen_asks_for_no_repaint() {
	for width in [390.0, 900.0] {
		let mut r = rig();
		for _ in 0..4 {
			r.frame(width);
		}
		assert_eq!(r.frame(width), Duration::MAX, "width {width}");
	}
}

#[tokio::test]
async fn the_window_hint_gates_meters() {
	let mut r = rig();
	r.frame(900.0);
	assert!(r.hub.visible());
	r.shell.visible.store(false, Ordering::SeqCst);
	r.frame(900.0);
	assert!(!r.hub.visible());
}

#[tokio::test]
async fn actions_go_through_the_shell() {
	let r = rig();
	r.actions.copy_text("relay://x");
	r.actions.hide_window();
	r.actions.quit_app();
	assert_eq!(*r.shell.copied.lock().unwrap(), ["relay://x"]);
	assert_eq!(r.shell.hides.load(Ordering::SeqCst), 1);
	assert_eq!(r.shell.quits.load(Ordering::SeqCst), 1);
	assert!(r.actions.shared.quitting.load(Ordering::SeqCst));
	assert!(!r.hub.visible());
}
