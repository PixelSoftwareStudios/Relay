//! Feasibility spike: relay-ui's compact layout in mock mode inside a
//! NativeActivity (eframe/glow). No engine, no audio, no network.
#![cfg(target_os = "android")]

use std::sync::Arc;

use android_activity::AndroidApp;
use relay_ui::actions::{Actions, Shared, ShellSink};
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::emitter::{self, Cadence};
use relay_ui::hub::Hub;
use relay_ui::shell::{BoxFuture, FilePicker, PickedFile, Shell};
use relay_ui::ui::RelayApp;
use relay_ui::mock;

struct AndroidShell {
	ctx: egui::Context,
}

impl FilePicker for AndroidShell {
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async { Err("image picker not implemented in the spike".to_owned()) })
	}
}

impl Shell for AndroidShell {
	fn copy_text(&self, text: &str) {
		log::info!("copy_text ({} bytes): no clipboard bridge in the spike", text.len());
	}
	fn request_repaint(&self) {
		self.ctx.request_repaint();
	}
	fn window_visible(&self) -> bool {
		true
	}
	fn show_window(&self) {}
	fn hide_window(&self) {}
	fn quit(&self) {}
	fn hides_to_tray(&self) -> bool {
		false
	}
}

struct SpikeApp {
	app: RelayApp,
	_runtime: tokio::runtime::Runtime,
}

impl eframe::App for SpikeApp {
	fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		self.app.begin_frame(ctx);
	}
	fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
		self.app.ui(ui);
	}
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
	android_logger::init_once(
		android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("relay"),
	);
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.worker_threads(2)
		.enable_all()
		.build()
		.expect("tokio runtime");
	let options = eframe::NativeOptions {
		android_app: Some(app),
		..Default::default()
	};
	let result = eframe::run_native(
		"Relay",
		options,
		Box::new(move |cc| {
			let ctx = cc.egui_ctx.clone();
			let shell: Arc<dyn Shell> = Arc::new(AndroidShell { ctx: ctx.clone() });
			let hub = Arc::new(Hub::default());
			let slot = Arc::new(SessionSlot::default());
			let shared = Arc::new(Shared::default());
			let controller = Arc::new(Controller::new(hub.clone(), slot.clone()));
			let actions = Actions {
				controller,
				hub: hub.clone(),
				shared: shared.clone(),
				runtime: runtime.handle().clone(),
				shell: shell.clone(),
			};
			let sink = Arc::new(ShellSink { shared, shell });
			runtime.spawn(emitter::run(hub.clone(), sink, Cadence::default()));
			runtime.spawn(mock::run(hub, slot));
			let app = RelayApp::new(actions, &ctx);
			Ok(Box::new(SpikeApp { app, _runtime: runtime }))
		}),
	);
	if let Err(e) = result {
		log::error!("eframe exited with error: {e}");
	}
}

/// Link probe: starts the real engine so opus, sqlite, cpal, mdns, snow and
/// tonic cannot be dead-stripped out of the cdylib. Never called by the app yet.
#[cfg(feature = "engine")]
#[unsafe(no_mangle)]
pub extern "C" fn relay_link_probe(dir: *const std::ffi::c_char) -> i32 {
	use relay_engine::daemon::{DaemonHandle, RunOptions, Transport};
	let dir = unsafe { std::ffi::CStr::from_ptr(dir) }.to_string_lossy().into_owned();
	let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
	rt.block_on(async {
		let mut o = RunOptions::new(relay_config::paths::Paths::under(&dir));
		o.transport = Transport::Unix(Some(std::path::Path::new(&dir).join("relay.sock")));
		match DaemonHandle::start(o).await {
			Ok(h) => { let _ = h.shutdown().await; 0 }
			Err(e) => { log::error!("engine start failed: {e}"); 1 }
		}
	})
}
