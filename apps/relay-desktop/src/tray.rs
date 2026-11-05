//! System tray icon (macOS and Windows) with idle, in-call and muted glyphs.
//! On Linux the app builds no tray: `tray-icon` needs a GTK main loop there,
//! which this client does not run, so closing the window quits the app.

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use relay_ui::actions::Actions;
use relay_ui::emitter::{TrayState, TraySummary};
use relay_ui::imaging::decode_for_texture;

/// Monochrome template glyphs. macOS draws the `@2x` pair on Retina screens
/// and tints template images itself; other platforms use the 1x glyphs.
const GLYPHS: [(TrayState, &[u8], &[u8]); 3] = [
	(
		TrayState::Idle,
		include_bytes!("../../../assets/icon/tray-idle.png"),
		include_bytes!("../../../assets/icon/tray-idle@2x.png"),
	),
	(
		TrayState::InCall,
		include_bytes!("../../../assets/icon/tray-call.png"),
		include_bytes!("../../../assets/icon/tray-call@2x.png"),
	),
	(
		TrayState::Muted,
		include_bytes!("../../../assets/icon/tray-muted.png"),
		include_bytes!("../../../assets/icon/tray-muted@2x.png"),
	),
];

/// The glyph bytes for a state on the current platform.
pub fn glyph(state: TrayState) -> &'static [u8] {
	let (_, one, two) = GLYPHS
		.iter()
		.find(|(s, _, _)| *s == state)
		.copied()
		.unwrap_or(GLYPHS[0]);
	if cfg!(target_os = "macos") { two } else { one }
}

fn icon_for(state: TrayState) -> Option<Icon> {
	let image = decode_for_texture(glyph(state))?;
	Icon::from_rgba(image.rgba, image.size[0] as u32, image.size[1] as u32).ok()
}

pub struct Tray {
	icon: TrayIcon,
	status: MenuItem,
	shown: TrayState,
}

impl Tray {
	/// Only called when the tray text changed (the emitter bounds the rate).
	pub fn apply(&mut self, summary: &TraySummary) {
		let _ = self.icon.set_tooltip(Some(&summary.tooltip));
		self.status.set_text(&summary.tooltip);
		if summary.icon != self.shown
			&& let Some(icon) = icon_for(summary.icon)
		{
			let _ = self.icon.set_icon_templated(Some(icon));
			self.shown = summary.icon;
		}
	}
}

/// Build the tray on the UI thread. Failure is non-fatal.
pub fn build(actions: &Actions) -> Option<Tray> {
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
		.with_icon_templated(icon_for(TrayState::Idle)?)
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
	Some(Tray {
		icon,
		status,
		shown: TrayState::Idle,
	})
}
