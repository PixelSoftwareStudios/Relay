use std::sync::Arc;

use relay_desktop::app::DesktopApp;
use relay_desktop::host::DesktopShell;
use relay_desktop::tray;
use relay_ui::actions::{Actions, Shared, ShellSink};
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::emitter::{self, Cadence};
use relay_ui::hub::Hub;
use relay_ui::imaging::decode_for_texture;
use relay_ui::messaging::link::MsgLink;
use relay_ui::relay_link::{RelayLink, resolve_endpoint};
use relay_ui::shell::Shell;
use relay_ui::ui::RelayApp;
use relay_ui::{messaging, mock, supervisor};

/// Embedded from the repo-root assets folder.
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/relay-mark-256.png");

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
	let window_icon = decode_for_texture(WINDOW_ICON_PNG).expect("embedded window icon decodes");

	let viewport = egui::ViewportBuilder::default()
		.with_title("Relay")
		.with_inner_size([520.0, 800.0])
		.with_min_inner_size([380.0, 480.0])
		.with_icon(egui::IconData {
			rgba: window_icon.rgba,
			width: window_icon.size[0] as u32,
			height: window_icon.size[1] as u32,
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
			let host = Arc::new(DesktopShell::new(ctx.clone()));
			let shell: Arc<dyn Shell> = host.clone();
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
			let sink = Arc::new(ShellSink {
				shared: shared.clone(),
				shell,
			});
			runtime.spawn(emitter::run(hub.clone(), sink, Cadence::default()));
			if mock_mode {
				runtime.spawn(mock::run(hub.clone(), slot));
				runtime.spawn(messaging::mock::run(
					shared.messages.clone(),
					actions.shell.clone(),
				));
			} else {
				let msg_engine = Arc::new(MsgLink::new(resolve_endpoint));
				runtime.spawn(messaging::supervisor::supervise(
					msg_engine,
					shared.messages.clone(),
					actions.shell.clone(),
				));
				let engine = Arc::new(RelayLink::new(resolve_endpoint));
				runtime.spawn(supervisor::supervise(
					engine,
					hub,
					slot,
					actions.shell.clone(),
					shared.autostart.clone(),
				));
			}
			let tray = tray::build(&actions);
			host.set_tray(tray.is_some());
			let app = RelayApp::new(actions.clone(), &ctx);
			Ok(Box::new(DesktopApp::new(app, actions, tray, runtime)))
		}),
	)
}
