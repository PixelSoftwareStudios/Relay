//! System tray icon (macOS and Windows). On Linux the app builds no tray:
//! `tray-icon` needs a GTK main loop there, which this client does not run,
//! so closing the window quits the app instead of hiding it.

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::emitter::TraySummary;
use crate::shell::Actions;

pub struct Tray {
	icon: TrayIcon,
	status: MenuItem,
}

impl Tray {
	/// Only called when the tray text changed (the emitter bounds the rate).
	pub fn apply(&self, summary: &TraySummary) {
		let _ = self.icon.set_tooltip(Some(&summary.tooltip));
		self.status.set_text(&summary.tooltip);
	}
}

/// Build the tray on the UI thread. Failure is non-fatal.
pub fn build(actions: &Actions, rgba: Vec<u8>, size: u32) -> Option<Tray> {
	if cfg!(not(any(target_os = "macos", target_os = "windows"))) {
		return None;
	}
	let status = MenuItem::with_id("status", "Relay", false, None);
	let show = MenuItem::with_id("show", "Show Relay", true, None);
	let quit = MenuItem::with_id("quit", "Quit this app (daemon keeps running)", true, None);
	let menu = Menu::new();
	menu.append_items(&[&status, &show, &quit]).ok()?;
	let icon = TrayIconBuilder::new()
		.with_tooltip("Relay")
		.with_menu(Box::new(menu))
		.with_menu_on_left_click(false)
		.with_icon(Icon::from_rgba(rgba, size, size).ok()?)
		.build()
		.ok()?;

	let a = actions.clone();
	MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id.0.as_str() {
		"show" => a.show_window(),
		// The tray can outlive a hidden window that never repaints, so exit directly.
		"quit" => std::process::exit(0),
		_ => {}
	}));
	let a = actions.clone();
	TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
		if let TrayIconEvent::Click {
			button: MouseButton::Left,
			button_state: MouseButtonState::Up,
			..
		} = event
		{
			a.show_window();
		}
	}));
	Some(Tray { icon, status })
}
