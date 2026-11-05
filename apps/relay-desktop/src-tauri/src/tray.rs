//! Tray icon and the Tauri-backed [`Sink`].

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::emitter::{Sink, TraySummary};
use crate::model::{AppState, Meters};

pub const STATE_EVENT: &str = "relay://state";
pub const METERS_EVENT: &str = "relay://meters";

/// Whether a tray icon exists; without one, closing the window quits the app
/// rather than hiding it where the user cannot get it back.
#[derive(Default)]
pub struct TrayPresence(AtomicBool);

impl TrayPresence {
	pub fn set(&self, present: bool) {
		self.0.store(present, Ordering::Release);
	}

	pub fn present(&self) -> bool {
		self.0.load(Ordering::Acquire)
	}
}

pub struct TauriSink {
	app: AppHandle,
	tray: Option<(TrayIcon, MenuItem<Wry>)>,
}

pub fn show_main_window(app: &AppHandle) {
	if let Some(window) = app.get_webview_window("main") {
		let _ = window.show();
		let _ = window.set_focus();
	}
}

/// Build the tray. Failure is non-fatal (some Linux desktops have none).
pub fn build_tray(app: &AppHandle) -> Option<(TrayIcon, MenuItem<Wry>)> {
	let status = MenuItemBuilder::with_id("status", "Relay")
		.enabled(false)
		.build(app)
		.ok()?;
	let show = MenuItemBuilder::with_id("show", "Show Relay")
		.build(app)
		.ok()?;
	let quit = MenuItemBuilder::with_id("quit", "Quit this app (daemon keeps running)")
		.build(app)
		.ok()?;
	let menu = MenuBuilder::new(app)
		.items(&[&status, &show, &quit])
		.build()
		.ok()?;
	let mut builder = TrayIconBuilder::with_id("relay")
		.tooltip("Relay")
		.menu(&menu)
		.show_menu_on_left_click(false)
		.on_menu_event(|app, event| match event.id().as_ref() {
			"show" => show_main_window(app),
			"quit" => app.exit(0),
			_ => {}
		})
		.on_tray_icon_event(|tray, event| {
			if let TrayIconEvent::Click {
				button: MouseButton::Left,
				button_state: MouseButtonState::Up,
				..
			} = event
			{
				show_main_window(tray.app_handle());
			}
		});
	if let Some(icon) = app.default_window_icon() {
		builder = builder.icon(icon.clone());
	}
	builder.build(app).ok().map(|tray| (tray, status))
}

impl TauriSink {
	pub fn new(app: AppHandle, tray: Option<(TrayIcon, MenuItem<Wry>)>) -> Self {
		Self { app, tray }
	}
}

impl Sink for TauriSink {
	fn state(&self, state: &AppState) {
		let _ = self.app.emit(STATE_EVENT, state);
	}

	fn meters(&self, meters: &Meters) {
		let _ = self.app.emit(METERS_EVENT, meters);
	}

	fn tray(&self, summary: &TraySummary) {
		if let Some((icon, status)) = &self.tray {
			let _ = icon.set_tooltip(Some(&summary.tooltip));
			let _ = status.set_text(&summary.tooltip);
		}
	}
}
