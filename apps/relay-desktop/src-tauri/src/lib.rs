//! Relay desktop host: a thin Tauri shell around the `relay-api` client.
//! No audio, PCM or inference lives here; the daemon owns all of it.

pub mod apply;
pub mod commands;
pub mod controller;
pub mod convert;
pub mod emitter;
pub mod engine;
pub mod hub;
pub mod model;
pub mod relay_link;
pub mod supervisor;
pub mod tray;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};

use controller::{Controller, SessionSlot};
use emitter::Cadence;
use hub::Hub;
use relay_link::{RelayLink, resolve_endpoint};
use tray::{TauriSink, TrayPresence};

pub fn run() {
	tracing_subscriber::fmt()
		.with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
		.init();

	let hub = Arc::new(Hub::default());
	let slot = Arc::new(SessionSlot::default());
	let controller = Arc::new(Controller::new(hub.clone(), slot.clone()));
	let presence = Arc::new(TrayPresence::default());

	tauri::Builder::default()
		.manage(hub.clone())
		.manage(controller)
		.manage(presence.clone())
		.setup(move |app| {
			let handle = app.handle().clone();
			let tray = tray::build_tray(&handle);
			presence.set(tray.is_some());
			let sink = Arc::new(TauriSink::new(handle, tray));
			tauri::async_runtime::spawn(emitter::run(hub.clone(), sink, Cadence::default()));
			let engine = Arc::new(RelayLink::new(resolve_endpoint));
			tauri::async_runtime::spawn(supervisor::supervise(engine, hub.clone(), slot.clone()));
			Ok(())
		})
		.on_window_event(|window, event| {
			// Closing the window hides it while a tray icon can bring it back;
			// either way only this app is affected, never the daemon.
			if let WindowEvent::CloseRequested { api, .. } = event
				&& window.state::<Arc<TrayPresence>>().present()
			{
				api.prevent_close();
				let _ = window.hide();
			}
		})
		.invoke_handler(tauri::generate_handler![
			commands::get_state,
			commands::get_meters,
			commands::start_call,
			commands::accept_call,
			commands::decline_call,
			commands::end_call,
			commands::set_transmit,
			commands::set_call_controls,
			commands::set_global_controls,
			commands::select_devices,
			commands::set_profile,
			commands::upload_avatar,
			commands::hide_window,
			commands::quit_app,
		])
		.run(tauri::generate_context!())
		.expect("failed to run the Relay desktop app");
}
