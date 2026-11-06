//! The GameActivity entry point: runs eframe on the activity's surface and
//! draws the UI that [`Host`](crate::host::Host) keeps up to date.

use std::sync::Arc;
use std::time::{Duration, Instant};

use android_activity::AndroidApp;
use relay_ui::ui::RelayApp;

use crate::host::{Host, host};

const MIC_DENIED: &str = "Microphone permission is denied, so you cannot be heard. \
Allow it in Android Settings > Apps > Relay > Permissions.";
const HOST_WAIT: Duration = Duration::from_secs(30);

struct App {
	app: RelayApp,
	host: Arc<Host>,
}

impl eframe::App for App {
	fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		if self.host.microphone_denied() && self.host.shared.error().is_none() {
			self.host.shared.set_error(MIC_DENIED.to_owned());
		}
		self.app.begin_frame(ctx);
	}

	fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
		self.app.ui(ui);
	}
}

/// Kotlin starts the engine from `onCreate`; the native thread may get here first.
fn wait_for_host() -> Option<Arc<Host>> {
	let started = Instant::now();
	while started.elapsed() < HOST_WAIT {
		if let Some(host) = host() {
			return Some(host);
		}
		std::thread::sleep(Duration::from_millis(20));
	}
	None
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
	crate::logging::init();
	let Some(host) = wait_for_host() else {
		tracing::error!("the engine was never started; nothing to draw");
		return;
	};
	let options = eframe::NativeOptions {
		android_app: Some(app),
		..Default::default()
	};
	let for_ui = host.clone();
	let result = eframe::run_native(
		"Relay",
		options,
		Box::new(move |cc| {
			let ctx = cc.egui_ctx.clone();
			for_ui.shell.set_context(Some(ctx.clone()));
			let app = RelayApp::new(for_ui.actions.clone(), &ctx);
			Ok(Box::new(App { app, host: for_ui }))
		}),
	);
	host.shell.set_context(None);
	if let Err(e) = result {
		tracing::error!("eframe exited with an error: {e}");
	}
}
