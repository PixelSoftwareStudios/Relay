use std::sync::Arc;

use eframe::egui;
use relay_desktop::controller::{Controller, SessionSlot};
use relay_desktop::emitter::{self, Cadence};
use relay_desktop::hub::Hub;
use relay_desktop::relay_link::{RelayLink, resolve_endpoint};
use relay_desktop::shell::{Actions, EguiSink, Shared};
use relay_desktop::ui::{self, RelayApp};
use relay_desktop::{mock, supervisor, tray};

/// Embedded from the repo-root assets folder.
const LOGO_PNG: &[u8] = include_bytes!("../../../assets/icon/relay-mark.png");

fn main() -> eframe::Result {
	tracing_subscriber::fmt()
		.with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
		.init();
	let mock_mode =
		std::env::args().any(|a| a == "--mock") || std::env::var_os("RELAY_DESKTOP_MOCK").is_some();
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.worker_threads(2)
		.thread_name("relay-desktop-io")
		.enable_all()
		.build()
		.expect("tokio runtime");
	let logo = ui::decode_logo(LOGO_PNG);

	let viewport = egui::ViewportBuilder::default()
		.with_title("Relay")
		.with_inner_size([520.0, 800.0])
		.with_min_inner_size([380.0, 480.0])
		.with_icon(egui::IconData {
			rgba: logo.rgba.clone(),
			width: logo.size,
			height: logo.size,
		});
	let options = eframe::NativeOptions {
		viewport,
		..Default::default()
	};

	eframe::run_native(
		"Relay",
		options,
		Box::new(move |cc| {
			let ctx = cc.egui_ctx.clone();
			let hub = Arc::new(Hub::default());
			let slot = Arc::new(SessionSlot::default());
			let shared = Arc::new(Shared::default());
			let controller = Arc::new(Controller::new(hub.clone(), slot.clone()));
			let actions = Actions {
				controller,
				hub: hub.clone(),
				shared: shared.clone(),
				runtime: runtime.handle().clone(),
				ctx: ctx.clone(),
			};
			let sink = Arc::new(EguiSink {
				shared: shared.clone(),
				ctx,
			});
			runtime.spawn(emitter::run(hub.clone(), sink, Cadence::default()));
			if mock_mode {
				runtime.spawn(mock::run(hub.clone(), slot));
			} else {
				let engine = Arc::new(RelayLink::new(resolve_endpoint));
				runtime.spawn(supervisor::supervise(engine, hub, slot));
			}
			let tray = tray::build(&actions, logo.rgba.clone(), logo.size);
			Ok(Box::new(RelayApp::new(actions, tray, runtime, &logo)))
		}),
	)
}
